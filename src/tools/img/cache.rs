//! The `img_optim.db` cache of already-optimized images.
//!
//! `images` records each content hash once with how thoroughly it was optimized;
//! `paths` maps a path plus its size and mtime onto a hash. A database next to
//! the images stores paths relative to itself; the global one stores them absolute.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{self, Component, Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::dirs;
use crate::sqlite::{Db, Mode, finish};
use crate::tools::img::mime::Mime;

pub const FILE_NAME: &str = "img_optim.db";

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS images(
        sha256 TEXT PRIMARY KEY,
        type TEXT,
        fast INTEGER DEFAULT 0,
        strip INTEGER DEFAULT 0,
        inserted_at REAL DEFAULT (unixepoch('now', 'subsec'))
    );
    CREATE TABLE IF NOT EXISTS paths(
        path TEXT PRIMARY KEY,
        sha256 TEXT,
        size INTEGER,
        mtime REAL,
        FOREIGN KEY(sha256) REFERENCES images(sha256)
    );
";

/// Size and mtime as the cache compares them.
#[derive(Clone, Copy, PartialEq)]
pub struct Stamp {
    pub size: u64,
    pub mtime: f64,
}

impl Stamp {
    pub fn of(path: &Path) -> Result<Self> {
        let metadata = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
        Ok(Stamp {
            size: metadata.len(),
            // The same arithmetic CPython uses for st_mtime, so old rows still compare equal.
            mtime: metadata.mtime() as f64 + 1e-9 * metadata.mtime_nsec() as f64,
        })
    }
}

/// How an image was optimized.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Level {
    pub fast: bool,
    pub strip: bool,
}

impl Level {
    const THOROUGH: Level = Level {
        fast: false,
        strip: true,
    };

    /// Whether work done at this level makes work at `wanted` unnecessary.
    pub fn covers(self, wanted: Level) -> bool {
        self == Level::THOROUGH || self == wanted
    }
}

#[derive(Clone)]
pub struct ImageRecord {
    pub sha256: String,
    pub mime: Option<String>,
    pub level: Level,
    pub inserted_at: Option<f64>,
}

impl ImageRecord {
    pub fn is_type(&self, mime: Mime) -> bool {
        self.mime.as_deref() == Some(mime.as_str())
    }

    /// Reads `sha256, type, fast, strip, inserted_at` starting at column `base`.
    fn from_row(row: &Row, base: usize) -> rusqlite::Result<Self> {
        Ok(ImageRecord {
            sha256: row.get(base)?,
            mime: row.get(base + 1)?,
            level: Level {
                fast: row.get(base + 2)?,
                strip: row.get(base + 3)?,
            },
            inserted_at: row.get(base + 4)?,
        })
    }
}

pub struct PathRecord {
    pub stamp: Stamp,
    pub image: ImageRecord,
}

pub struct Cache {
    path: PathBuf,
    relative_paths: bool,
    db: Db,
    modified: bool,
}

impl Cache {
    pub fn open(path: &Path, relative_paths: bool) -> Result<Self> {
        let path = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let db = Db::open(&path, Mode::ReadWrite)?;
        db.connection
            .execute_batch(SCHEMA)
            .with_context(|| format!("opening {}", path.display()))?;
        Ok(Cache {
            path,
            relative_paths,
            db,
            modified: false,
        })
    }

    pub fn open_read_only(path: &Path) -> Result<Self> {
        let path = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        let db = Db::open(&path, Mode::ReadOnly)?;
        Ok(Cache {
            path,
            relative_paths: false,
            db,
            modified: false,
        })
    }

    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(&self.path)
    }

    fn key(&self, path: &Path) -> Result<String> {
        let absolute =
            path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        let key = if self.relative_paths {
            relative_to(&absolute, self.dir())
        } else {
            absolute
        };
        Ok(key.to_string_lossy().into_owned())
    }

    /// Forgets paths that no longer exist.
    pub fn prune(&mut self) -> Result<()> {
        let keys = self
            .db
            .connection
            .prepare("SELECT path FROM paths")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let missing: Vec<String> = keys
            .into_iter()
            .filter(|key| {
                let path = Path::new(key);
                let path = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    self.dir().join(path)
                };
                !path.exists()
            })
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let transaction = self.db.connection.transaction()?;
        {
            let mut statement = transaction.prepare_cached("DELETE FROM paths WHERE path = ?1")?;
            for key in &missing {
                statement.execute(params![key.as_str()])?;
            }
        }
        transaction.commit()?;
        self.modified = true;
        Ok(())
    }

    pub fn get(&self, path: &Path) -> Result<Option<PathRecord>> {
        let key = self.key(path)?;
        let record = self
            .db
            .connection
            .query_row(
                "SELECT p.size, p.mtime, i.sha256, i.type, i.fast, i.strip, i.inserted_at
                FROM paths p
                JOIN images i ON p.sha256 = i.sha256
                WHERE p.path = ?1",
                params![key],
                |row| {
                    Ok(PathRecord {
                        stamp: Stamp {
                            size: row.get(0)?,
                            mtime: row.get(1)?,
                        },
                        image: ImageRecord::from_row(row, 2)?,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }

    pub fn get_by_sha256(&self, sha256: &str) -> Result<Option<ImageRecord>> {
        let record = self
            .db
            .connection
            .query_row(
                "SELECT sha256, type, fast, strip, inserted_at FROM images WHERE sha256 = ?1",
                params![sha256],
                |row| ImageRecord::from_row(row, 0),
            )
            .optional()?;
        Ok(record)
    }

    pub fn delete_path(&mut self, path: &Path) -> Result<()> {
        let key = self.key(path)?;
        let changed = self
            .db
            .connection
            .execute("DELETE FROM paths WHERE path = ?1", params![key])?;
        self.modified |= changed > 0;
        Ok(())
    }

    pub fn delete_paths_under(&mut self, directory: &Path) -> Result<()> {
        let prefix = format!("{}/", directory.display());
        let changed = self.db.connection.execute(
            "DELETE FROM paths WHERE substr(path, 1, ?1) = ?2",
            params![prefix.chars().count() as i64, prefix.as_str()],
        )?;
        self.modified |= changed > 0;
        Ok(())
    }

    /// Records an image seen in another database, keeping any existing row.
    pub fn backfill_image(&mut self, image: &ImageRecord) -> Result<()> {
        self.db.connection.execute(
            "INSERT INTO images(sha256, type, fast, strip, inserted_at)
            VALUES(?1, ?2, ?3, ?4, ?5)
            ON CONFLICT DO NOTHING",
            params![
                image.sha256.as_str(),
                image.mime.as_deref(),
                image.level.fast,
                image.level.strip,
                image.inserted_at,
            ],
        )?;
        self.modified = true;
        Ok(())
    }

    pub fn upsert_path(&mut self, path: &Path, sha256: &str, stamp: Stamp) -> Result<()> {
        let key = self.key(path)?;
        upsert_path(&self.db.connection, &key, sha256, stamp)?;
        self.modified = true;
        Ok(())
    }

    pub fn upsert(
        &mut self,
        path: &Path,
        stamp: Stamp,
        mime: Mime,
        sha256: &str,
        level: Level,
    ) -> Result<()> {
        let key = self.key(path)?;
        let transaction = self.db.connection.transaction()?;
        transaction.execute(
            "INSERT INTO images(sha256, type, fast, strip, inserted_at)
            VALUES(?1, ?2, ?3, ?4, unixepoch('now', 'subsec'))
            ON CONFLICT DO UPDATE
                SET type = excluded.type,
                    fast = excluded.fast,
                    strip = excluded.strip,
                    inserted_at = unixepoch('now', 'subsec')",
            params![sha256, mime.as_str(), level.fast, level.strip],
        )?;
        upsert_path(&transaction, &key, sha256, stamp)?;
        transaction.commit()?;
        self.modified = true;
        Ok(())
    }

    fn close(self, vacuum: bool) -> Result<()> {
        let vacuumed = if vacuum && self.modified {
            self.db
                .connection
                .execute_batch("VACUUM")
                .with_context(|| format!("closing {}", self.path.display()))
        } else {
            Ok(())
        };
        finish(vacuumed, self.db.close())
    }
}

fn upsert_path(
    connection: &Connection,
    key: &str,
    sha256: &str,
    stamp: Stamp,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO paths(path, sha256, size, mtime)
        VALUES(?1, ?2, ?3, ?4)
        ON CONFLICT DO UPDATE
            SET sha256 = excluded.sha256,
                size = excluded.size,
                mtime = excluded.mtime",
        params![key, sha256, stamp.size, stamp.mtime],
    )?;
    Ok(())
}

/// `path` relative to `base`, both absolute, climbing with `..` where needed.
fn relative_to(path: &Path, base: &Path) -> PathBuf {
    let mut path_parts = path.components().peekable();
    let mut base_parts = base.components().peekable();
    while let (Some(a), Some(b)) = (path_parts.peek(), base_parts.peek()) {
        if a != b {
            break;
        }
        path_parts.next();
        base_parts.next();
    }
    let mut relative = PathBuf::new();
    for _ in base_parts {
        relative.push(Component::ParentDir);
    }
    for part in path_parts {
        relative.push(part);
    }
    if relative.as_os_str().is_empty() {
        relative.push(Component::CurDir);
    }
    relative
}

/// The primary cache with the global cache and read-only hash databases behind it.
pub struct Broker {
    primary: Cache,
    secondary: Option<Cache>,
    hash_dbs: Vec<Cache>,
    cleanup: bool,
}

impl Broker {
    /// Builds a broker over the caches, runs `body` on it, and always closes it.
    pub fn with<R>(
        primary: Cache,
        secondary: Option<Cache>,
        hash_dbs: Vec<Cache>,
        body: impl FnOnce(&mut Broker) -> Result<R>,
    ) -> Result<R> {
        let mut broker = Broker::new(primary, secondary, hash_dbs)?;
        let result = body(&mut broker);
        finish(result, broker.close())
    }

    fn new(primary: Cache, mut secondary: Option<Cache>, hash_dbs: Vec<Cache>) -> Result<Self> {
        if let Some(secondary) = &mut secondary {
            secondary.prune()?;
        }
        Ok(Broker {
            primary,
            secondary,
            hash_dbs,
            cleanup: true,
        })
    }

    pub fn get(&self, path: &Path) -> Result<Option<PathRecord>> {
        self.primary.get(path)
    }

    /// Looks the hash up everywhere, copying a hit from elsewhere into the primary.
    pub fn get_by_sha256(&mut self, sha256: &str) -> Result<Option<ImageRecord>> {
        if let Some(record) = self.primary.get_by_sha256(sha256)? {
            return Ok(Some(record));
        }
        for source in self.secondary.iter().chain(&self.hash_dbs) {
            if let Some(record) = source.get_by_sha256(sha256)? {
                self.primary.backfill_image(&record)?;
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    pub fn upsert(
        &mut self,
        path: &Path,
        stamp: Stamp,
        mime: Mime,
        sha256: &str,
        level: Level,
    ) -> Result<()> {
        self.evict_secondary(path)?;
        self.primary.upsert(path, stamp, mime, sha256, level)
    }

    pub fn upsert_path(&mut self, path: &Path, sha256: &str, stamp: Stamp) -> Result<()> {
        self.evict_secondary(path)?;
        self.primary.upsert_path(path, sha256, stamp)
    }

    fn evict_secondary(&mut self, path: &Path) -> Result<()> {
        match &mut self.secondary {
            Some(secondary) => secondary.delete_path(path),
            None => Ok(()),
        }
    }

    /// Skips the vacuum and global-cache cleanup on close, as after an interrupt.
    pub fn skip_cleanup(&mut self) {
        self.cleanup = false;
    }

    /// Closes everything, even after a failure; cleanup also vacuums and drops the
    /// primary's tree from the global cache.
    fn close(self) -> Result<()> {
        let cleanup = self.cleanup;
        let primary_dir = self.primary.dir().to_path_buf();
        let mut result = self.primary.close(cleanup);
        if let Some(mut secondary) = self.secondary {
            if cleanup {
                result = finish(result, secondary.delete_paths_under(&primary_dir));
            }
            result = finish(result, secondary.close(cleanup));
        }
        for db in self.hash_dbs {
            result = finish(result, db.close(false));
        }
        result
    }
}

pub fn global_path() -> Result<PathBuf> {
    Ok(dirs::cache()?.join(FILE_NAME))
}

/// The database to use: the nearest one above the inputs' common directory,
/// else the global one, or with `create` a new one in the common directory.
pub fn locate(paths: &[PathBuf], create: bool) -> Result<PathBuf> {
    let mut dirs = Vec::new();
    for path in paths {
        let absolute =
            path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        dirs.push(if absolute.is_dir() {
            absolute
        } else {
            absolute.parent().map(Path::to_path_buf).unwrap_or(absolute)
        });
    }
    let common = common_path(&dirs);
    for dir in common.ancestors() {
        let candidate = dir.join(FILE_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    if create {
        Ok(common.join(FILE_NAME))
    } else {
        global_path()
    }
}

fn common_path(paths: &[PathBuf]) -> PathBuf {
    let Some((first, rest)) = paths.split_first() else {
        return PathBuf::from("/");
    };
    let mut common: Vec<Component<'_>> = first.components().collect();
    for path in rest {
        let shared = common
            .iter()
            .zip(path.components())
            .take_while(|(a, b)| *a == b)
            .count();
        common.truncate(shared);
    }
    common.iter().collect()
}
