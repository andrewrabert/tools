//! SQLite databases in rollback-journal (`DELETE`) mode.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};

pub enum Mode {
    ReadWrite,
    ReadOnly,
}

pub struct Db {
    pub connection: Connection,
    path: PathBuf,
}

impl Db {
    pub fn open(path: &Path, mode: Mode) -> Result<Db> {
        let connection = match mode {
            Mode::ReadWrite => Connection::open(path).and_then(|connection| {
                connection.pragma_update(None, "journal_mode", "DELETE")?;
                Ok(connection)
            }),
            Mode::ReadOnly => Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ),
        }
        .with_context(|| format!("opening {}", path.display()))?;
        Ok(Db {
            connection,
            path: path.to_path_buf(),
        })
    }

    /// Closes the database, returning any close error.
    pub fn close(self) -> Result<()> {
        self.connection
            .close()
            .map_err(|(_, error)| error)
            .with_context(|| format!("closing {}", self.path.display()))
    }
}

/// Combines a scoped body's result with its close result so neither error is lost.
pub fn finish<R>(body: Result<R>, close: Result<()>) -> Result<R> {
    match (body, close) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(close)) => Err(close),
        (Err(body), Ok(())) => Err(body),
        (Err(body), Err(close)) => Err(anyhow::anyhow!("{body:#}\nclosing also failed: {close:#}")),
    }
}
