//! The `img_optim.db` cache of already-optimized images.
//!
//! `images` records each content hash once with how thoroughly it was optimized;
//! `paths` maps a path plus its size and mtime onto a hash. A database next to
//! the images stores paths relative to itself; the global one stores them absolute.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{self, Component, Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, params};

use crate::dirs;
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

    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(ImageRecord {
            sha256: row.get("sha256")?,
            mime: row.get("type")?,
            level: Level {
                fast: row.get::<_, i64>("fast")? != 0,
                strip: row.get::<_, i64>("strip")? != 0,
            },
            inserted_at: row.get("inserted_at")?,
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
    connection: Connection,
    modified: bool,
}

impl Cache {
    pub fn open(path: &Path, relative_paths: bool) -> Result<Self> {
        let path = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let connection =
            Connection::open(&path).with_context(|| format!("opening {}", path.display()))?;
        connection.execute_batch(SCHEMA)?;
        Ok(Cache {
            path,
            relative_paths,
            connection,
            modified: false,
        })
    }

    pub fn open_read_only(path: &Path) -> Result<Self> {
        let path = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening {}", path.display()))?;
        Ok(Cache {
            path,
            relative_paths: false,
            connection,
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
        let mut statement = self.connection.prepare("SELECT path FROM paths")?;
        let keys = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);

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
        let transaction = self.connection.transaction()?;
        for key in &missing {
            transaction.execute("DELETE FROM paths WHERE path = ?1", params![key])?;
        }
        transaction.commit()?;
        self.modified = true;
        Ok(())
    }

    pub fn get(&self, path: &Path) -> Result<Option<PathRecord>> {
        let key = self.key(path)?;
        let record = self
            .connection
            .query_row(
                "SELECT p.size, p.mtime, i.sha256, i.type, i.fast, i.strip, i.inserted_at
                FROM paths p
                JOIN images i ON p.sha256 = i.sha256
                WHERE p.path = ?1",
                params![key],
                |row| {
                    let size: i64 = row.get("size")?;
                    Ok(PathRecord {
                        stamp: Stamp {
                            size: u64::try_from(size).unwrap_or_default(),
                            mtime: row.get("mtime")?,
                        },
                        image: ImageRecord::from_row(row)?,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }

    pub fn get_by_sha256(&self, sha256: &str) -> Result<Option<ImageRecord>> {
        let record = self
            .connection
            .query_row(
                "SELECT sha256, type, fast, strip, inserted_at FROM images WHERE sha256 = ?1",
                params![sha256],
                ImageRecord::from_row,
            )
            .optional()?;
        Ok(record)
    }

    pub fn delete_path(&mut self, path: &Path) -> Result<()> {
        let key = self.key(path)?;
        let changed = self
            .connection
            .execute("DELETE FROM paths WHERE path = ?1", params![key])?;
        self.modified |= changed > 0;
        Ok(())
    }

    pub fn delete_paths_under(&mut self, directory: &Path) -> Result<()> {
        let prefix = format!("{}/", directory.display());
        let changed = self.connection.execute(
            "DELETE FROM paths WHERE substr(path, 1, ?1) = ?2",
            params![prefix.chars().count() as i64, prefix],
        )?;
        self.modified |= changed > 0;
        Ok(())
    }

    /// Records an image seen in another database, keeping any existing row.
    pub fn backfill_image(&mut self, image: &ImageRecord) -> Result<()> {
        self.connection.execute(
            "INSERT INTO images(sha256, type, fast, strip, inserted_at)
            VALUES(?1, ?2, ?3, ?4, ?5)
            ON CONFLICT DO NOTHING",
            params![
                image.sha256,
                image.mime,
                i64::from(image.level.fast),
                i64::from(image.level.strip),
                image.inserted_at,
            ],
        )?;
        self.modified = true;
        Ok(())
    }

    pub fn upsert_path(&mut self, path: &Path, sha256: &str, stamp: Stamp) -> Result<()> {
        let key = self.key(path)?;
        upsert_path(&self.connection, &key, sha256, stamp)?;
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
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO images(sha256, type, fast, strip, inserted_at)
            VALUES(?1, ?2, ?3, ?4, unixepoch('now', 'subsec'))
            ON CONFLICT DO UPDATE
                SET type = excluded.type,
                    fast = excluded.fast,
                    strip = excluded.strip,
                    inserted_at = unixepoch('now', 'subsec')",
            params![
                sha256,
                mime.as_str(),
                i64::from(level.fast),
                i64::from(level.strip)
            ],
        )?;
        upsert_path(&transaction, &key, sha256, stamp)?;
        transaction.commit()?;
        self.modified = true;
        Ok(())
    }

    pub fn close(self, vacuum: bool) -> Result<()> {
        if vacuum && self.modified {
            self.connection.execute_batch("VACUUM")?;
        }
        self.connection
            .close()
            .map_err(|(_, error)| error)
            .with_context(|| format!("closing {}", self.path.display()))
    }
}

fn upsert_path(connection: &Connection, key: &str, sha256: &str, stamp: Stamp) -> Result<()> {
    connection.execute(
        "INSERT INTO paths(path, sha256, size, mtime)
        VALUES(?1, ?2, ?3, ?4)
        ON CONFLICT DO UPDATE
            SET sha256 = excluded.sha256,
                size = excluded.size,
                mtime = excluded.mtime",
        params![
            key,
            sha256,
            i64::try_from(stamp.size).context("file is too large to record")?,
            stamp.mtime
        ],
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
}

impl Broker {
    pub fn new(primary: Cache, mut secondary: Option<Cache>, hash_dbs: Vec<Cache>) -> Result<Self> {
        if let Some(secondary) = &mut secondary {
            secondary.prune()?;
        }
        Ok(Broker {
            primary,
            secondary,
            hash_dbs,
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

    /// Closes everything; `cleanup` also vacuums and drops the primary's tree from the global cache.
    pub fn close(self, cleanup: bool) -> Result<()> {
        let primary_dir = self.primary.dir().to_path_buf();
        self.primary.close(cleanup)?;
        if let Some(mut secondary) = self.secondary {
            if cleanup {
                secondary.delete_paths_under(&primary_dir)?;
            }
            secondary.close(cleanup)?;
        }
        for db in self.hash_dbs {
            db.close(false)?;
        }
        Ok(())
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
