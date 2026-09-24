use std::collections::BTreeMap;
use std::path::{self, Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Row, params, params_from_iter};

use crate::sqlite::{Db, Mode, finish};
use crate::tools::file_hash_recorder::hash::Hashes;
use crate::tools::file_hash_recorder::record::{FileRecord, Fingerprint, Stamp};

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS files(
        path,
        crc32,
        md5,
        mtime real,
        sha1,
        sha256,
        size int,

        PRIMARY KEY(path)
    );
    CREATE TABLE IF NOT EXISTS archive_contents(
        parent,
        path,
        crc32,
        md5,
        mtime real,
        sha1,
        sha256,
        size int,

        PRIMARY KEY(parent, path),

        FOREIGN KEY(parent)
            REFERENCES FILES(path)
            ON DELETE CASCADE
            ON UPDATE CASCADE
    );
";

const FINGERPRINT_COLUMNS: &str = "crc32, md5, mtime, sha1, sha256, size";

pub enum Scope {
    All,
    Under(String),
    File(String),
    Outside,
}

pub struct FileInfoDb {
    path: PathBuf,
    db: Db,
    modified: bool,
}

impl FileInfoDb {
    /// Opens the database at `path`, runs `body` on it, and always closes it.
    pub fn with<R>(path: &Path, body: impl FnOnce(&mut FileInfoDb) -> Result<R>) -> Result<R> {
        let mut db = Self::open(path)?;
        let result = body(&mut db);
        finish(result, db.close())
    }

    fn open(path: &Path) -> Result<Self> {
        let path = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        let db = Db::open(&path, Mode::ReadWrite)?;
        db.connection
            .execute_batch("PRAGMA foreign_keys = ON")
            .and_then(|()| db.connection.execute_batch(SCHEMA))
            .with_context(|| format!("opening {}", path.display()))?;
        Ok(FileInfoDb {
            path,
            db,
            modified: false,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(&self.path)
    }

    pub fn records(&self, scope: &Scope) -> Result<BTreeMap<String, FileRecord>> {
        let (filter, keys) = match scope {
            Scope::All => ("", Vec::new()),
            Scope::Under(prefix) => ("WHERE substr({column}, 1, length(?1)) = ?1", vec![prefix]),
            Scope::File(name) => ("WHERE {column} = ?1", vec![name]),
            Scope::Outside => return Ok(BTreeMap::new()),
        };
        let connection = &self.db.connection;
        let mut records = BTreeMap::new();
        let sql = format!(
            "SELECT path, {FINGERPRINT_COLUMNS} FROM files {}",
            filter.replace("{column}", "path")
        );
        let mut statement = connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(&keys))?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(0)?;
            let record = FileRecord {
                fingerprint: fingerprint(row, 1)?,
                archive_contents: BTreeMap::new(),
            };
            records.insert(name, record);
        }

        let sql = format!(
            "SELECT parent, path, {FINGERPRINT_COLUMNS} FROM archive_contents {}",
            filter.replace("{column}", "parent")
        );
        let mut statement = connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(&keys))?;
        while let Some(row) = rows.next()? {
            let parent: String = row.get(0)?;
            let path: String = row.get(1)?;
            let entry = fingerprint(row, 2)?;
            let record = records
                .get_mut(&parent)
                .with_context(|| format!("archive contents recorded for unknown file {parent}"))?;
            record.archive_contents.insert(path, entry);
        }
        Ok(records)
    }

    pub fn upsert(&mut self, name: &str, record: &FileRecord) -> Result<()> {
        let transaction = self.db.connection.transaction()?;
        let file = &record.fingerprint;
        let mut changed = transaction.execute(
            "INSERT INTO files(path, crc32, md5, mtime, sha1, sha256, size)
            VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(path) DO UPDATE
                SET crc32 = excluded.crc32,
                    md5 = excluded.md5,
                    mtime = excluded.mtime,
                    sha1 = excluded.sha1,
                    sha256 = excluded.sha256,
                    size = excluded.size",
            params![
                name,
                file.hashes.crc32.as_str(),
                file.hashes.md5.as_str(),
                file.stamp.mtime,
                file.hashes.sha1.as_str(),
                file.hashes.sha256.as_str(),
                file.stamp.size,
            ],
        )?;
        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO archive_contents(parent, path, crc32, md5, mtime, sha1, sha256, size)
                VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(parent, path) DO UPDATE
                    SET crc32 = excluded.crc32,
                        md5 = excluded.md5,
                        mtime = excluded.mtime,
                        sha1 = excluded.sha1,
                        sha256 = excluded.sha256,
                        size = excluded.size",
            )?;
            for (entry_name, entry) in &record.archive_contents {
                changed += statement.execute(params![
                    name,
                    entry_name.as_str(),
                    entry.hashes.crc32.as_str(),
                    entry.hashes.md5.as_str(),
                    entry.stamp.mtime,
                    entry.hashes.sha1.as_str(),
                    entry.hashes.sha256.as_str(),
                    entry.stamp.size,
                ])?;
            }
        }
        transaction.commit()?;
        self.modified |= changed > 0;
        Ok(())
    }

    pub fn delete(&mut self, name: &str) -> Result<()> {
        let changed = self
            .db
            .connection
            .execute("DELETE FROM files WHERE path = ?1", params![name])?;
        self.modified |= changed > 0;
        Ok(())
    }

    fn close(self) -> Result<()> {
        let vacuumed = if self.modified {
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

/// The fingerprint in the `FINGERPRINT_COLUMNS` starting at column `base`.
fn fingerprint(row: &Row, base: usize) -> Result<Fingerprint> {
    Ok(Fingerprint {
        hashes: Hashes {
            crc32: row.get(base)?,
            md5: row.get(base + 1)?,
            sha1: row.get(base + 3)?,
            sha256: row.get(base + 4)?,
        },
        stamp: Stamp {
            mtime: row.get(base + 2)?,
            size: row.get(base + 5)?,
        },
    })
}
