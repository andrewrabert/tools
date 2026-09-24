use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::named_params;

use crate::sqlite::{Db, Mode, finish};

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS files(
        path,
        mtime real,
        size int,
        mkv_info_json,

        PRIMARY KEY(path)
    );
    CREATE TABLE IF NOT EXISTS overrides(
        path TEXT NOT NULL PRIMARY KEY,
        langs_json TEXT NOT NULL
    );
";

/// Matches the recorded path `:rel` and every path under it.
const UNDER: &str = "WHERE :rel = '' OR path = :rel OR substr(path, 1, length(:prefix)) = :prefix";

pub struct FileInfo {
    pub mtime: f64,
    pub size: u64,
    pub mkv_info_json: String,
}

pub type Languages = BTreeSet<String>;

/// A mkv_info.db, which records paths relative to its directory.
pub struct MkvInfoDb {
    root: PathBuf,
    db: Db,
}

impl MkvInfoDb {
    /// Opens the database at `path`, runs `body` on it, and always closes it.
    /// `path` must be absolute and normalized.
    pub fn with<R>(path: &Path, body: impl FnOnce(&MkvInfoDb) -> Result<R>) -> Result<R> {
        let db = Self::open(path)?;
        let result = body(&db);
        finish(result, db.close())
    }

    fn close(self) -> Result<()> {
        self.db.close()
    }

    /// `path` must be absolute and normalized.
    fn open(path: &Path) -> Result<Self> {
        let root = path.parent().context("database has no parent directory")?;
        let db = Db::open(path, Mode::ReadWrite)?;
        db.connection
            .execute_batch("PRAGMA foreign_keys = ON")
            .and_then(|()| db.connection.execute_batch(SCHEMA))
            .with_context(|| format!("opening {}", path.display()))?;
        Ok(MkvInfoDb {
            root: root.to_path_buf(),
            db,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn relative(&self, path: &Path) -> Result<String> {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            bail!(
                "path outside of {}: {}",
                self.root.display(),
                path.display()
            );
        };
        let parts = relative
            .iter()
            .map(|part| {
                part.to_str()
                    .with_context(|| format!("path is not UTF-8: {}", path.display()))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(parts.join("/"))
    }

    /// The recorded files at and under `path`, by absolute path.
    pub fn files_under(&self, path: &Path) -> Result<BTreeMap<PathBuf, FileInfo>> {
        let relative = self.relative(path)?;
        let prefix = format!("{relative}/");
        let mut statement = self.db.connection.prepare(&format!(
            "SELECT path, mtime, size, mkv_info_json FROM files {UNDER}"
        ))?;
        let mut rows = statement.query(named_params! {
            ":rel": relative,
            ":prefix": prefix,
        })?;
        let mut files = BTreeMap::new();
        while let Some(row) = rows.next()? {
            let name: String = row.get(0)?;
            let info = FileInfo {
                mtime: row.get(1)?,
                size: row.get(2)?,
                mkv_info_json: row.get(3)?,
            };
            files.insert(self.root.join(name), info);
        }
        Ok(files)
    }

    pub fn upsert_file(&self, path: &Path, info: &FileInfo) -> Result<()> {
        let relative = self.relative(path)?;
        self.db.connection.execute(
            "INSERT INTO files(path, mtime, size, mkv_info_json)
                VALUES(:path, :mtime, :size, :mkv_info_json)
                ON CONFLICT DO UPDATE
                    SET mtime = :mtime,
                        mkv_info_json = :mkv_info_json,
                        size = :size",
            named_params! {
                ":path": relative,
                ":mtime": info.mtime,
                ":size": info.size,
                ":mkv_info_json": info.mkv_info_json.as_str(),
            },
        )?;
        Ok(())
    }

    pub fn delete_file(&self, path: &Path) -> Result<()> {
        let relative = self.relative(path)?;
        self.db.connection.execute(
            "DELETE FROM files WHERE path = :path",
            named_params! { ":path": relative },
        )?;
        Ok(())
    }

    /// The overrides at and under `path`, by absolute path.
    pub fn overrides_under(&self, path: &Path) -> Result<BTreeMap<PathBuf, Languages>> {
        let relative = self.relative(path)?;
        let prefix = format!("{relative}/");
        let mut statement = self.db.connection.prepare(&format!(
            "SELECT path, langs_json FROM overrides {UNDER} ORDER BY path"
        ))?;
        let mut rows = statement.query(named_params! {
            ":rel": relative,
            ":prefix": prefix,
        })?;
        let mut overrides = BTreeMap::new();
        while let Some(row) = rows.next()? {
            let name: String = row.get(0)?;
            let json: String = row.get(1)?;
            let languages = serde_json::from_str(&json)
                .with_context(|| format!("parsing the override languages of {name}"))?;
            overrides.insert(self.root.join(name), languages);
        }
        Ok(overrides)
    }

    /// Replaces the override of `path`, removing it when `languages` is empty.
    pub fn set_override(&self, path: &Path, languages: &Languages) -> Result<()> {
        let relative = self.relative(path)?;
        if languages.is_empty() {
            self.db.connection.execute(
                "DELETE FROM overrides WHERE path = :path",
                named_params! { ":path": relative },
            )?;
        } else {
            let langs_json = serde_json::to_string(languages)?;
            self.db.connection.execute(
                "INSERT INTO overrides(path, langs_json)
                    VALUES(:path, :langs_json)
                    ON CONFLICT DO UPDATE SET langs_json = :langs_json",
                named_params! {
                    ":path": relative,
                    ":langs_json": langs_json,
                },
            )?;
        }
        Ok(())
    }
}
