use std::collections::BTreeMap;
use std::path::{self, Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, Row, params, params_from_iter};

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
    connection: Connection,
    modified: bool,
}

impl FileInfoDb {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        let connection =
            Connection::open(&path).with_context(|| format!("opening {}", path.display()))?;
        connection.execute_batch("PRAGMA foreign_keys = ON")?;
        connection.execute_batch(SCHEMA)?;
        Ok(FileInfoDb {
            path,
            connection,
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

        let mut records = BTreeMap::new();
        let sql = format!(
            "SELECT path, {FINGERPRINT_COLUMNS} FROM files {}",
            filter.replace("{column}", "path")
        );
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(&keys))?;
        while let Some(row) = rows.next()? {
            let record = FileRecord {
                fingerprint: fingerprint(row, 1)?,
                archive_contents: BTreeMap::new(),
            };
            records.insert(row.get(0)?, record);
        }

        let sql = format!(
            "SELECT parent, path, {FINGERPRINT_COLUMNS} FROM archive_contents {}",
            filter.replace("{column}", "parent")
        );
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(&keys))?;
        while let Some(row) = rows.next()? {
            let parent: String = row.get(0)?;
            let record = records
                .get_mut(&parent)
                .with_context(|| format!("archive contents recorded for unknown file {parent}"))?;
            record
                .archive_contents
                .insert(row.get(1)?, fingerprint(row, 2)?);
        }
        Ok(records)
    }

    pub fn upsert(&mut self, name: &str, record: &FileRecord) -> Result<()> {
        let transaction = self.connection.transaction()?;
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
                file.hashes.crc32,
                file.hashes.md5,
                file.stamp.mtime,
                file.hashes.sha1,
                file.hashes.sha256,
                sql_size(file.stamp.size)?,
            ],
        )?;
        for (entry_name, entry) in &record.archive_contents {
            changed += transaction.execute(
                "INSERT INTO archive_contents(parent, path, crc32, md5, mtime, sha1, sha256, size)
                VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(parent, path) DO UPDATE
                    SET crc32 = excluded.crc32,
                        md5 = excluded.md5,
                        mtime = excluded.mtime,
                        sha1 = excluded.sha1,
                        sha256 = excluded.sha256,
                        size = excluded.size",
                params![
                    name,
                    entry_name,
                    entry.hashes.crc32,
                    entry.hashes.md5,
                    entry.stamp.mtime,
                    entry.hashes.sha1,
                    entry.hashes.sha256,
                    sql_size(entry.stamp.size)?,
                ],
            )?;
        }
        transaction.commit()?;
        self.modified |= changed > 0;
        Ok(())
    }

    pub fn delete(&mut self, name: &str) -> Result<()> {
        let changed = self
            .connection
            .execute("DELETE FROM files WHERE path = ?1", params![name])?;
        self.modified |= changed > 0;
        Ok(())
    }

    pub fn close(self) -> Result<()> {
        if self.modified {
            self.connection.execute_batch("VACUUM")?;
        }
        self.connection
            .close()
            .map_err(|(_, error)| error)
            .with_context(|| format!("closing {}", self.path.display()))
    }
}

fn fingerprint(row: &Row<'_>, first_column: usize) -> Result<Fingerprint> {
    let size: i64 = row.get(first_column + 5)?;
    Ok(Fingerprint {
        hashes: Hashes {
            crc32: row.get(first_column)?,
            md5: row.get(first_column + 1)?,
            sha1: row.get(first_column + 3)?,
            sha256: row.get(first_column + 4)?,
        },
        stamp: Stamp {
            mtime: row.get(first_column + 2)?,
            size: u64::try_from(size).context("negative size in the database")?,
        },
    })
}

fn sql_size(size: u64) -> Result<i64> {
    i64::try_from(size).context("file is too large to record")
}
