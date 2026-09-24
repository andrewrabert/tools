//! Files written to a temporary sibling, then moved over their destination.
use std::ffi::OsStr;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result};
use tempfile::{Builder, TempPath};

const DEFAULT_MODE: u32 = 0o666;

pub struct Sibling<'a> {
    dir: &'a Path,
    prefix: &'a str,
    path: &'a Path,
    mode: u32,
}

/// A temporary file in `dir` named after `path` plus `.tmp`.
pub fn sibling<'a>(dir: &'a Path, prefix: &'a str, path: &'a Path) -> Sibling<'a> {
    Sibling {
        dir,
        prefix,
        path,
        mode: DEFAULT_MODE,
    }
}

impl Sibling<'_> {
    pub fn create(self) -> Result<TempPath> {
        let mut suffix = self
            .path
            .file_name()
            .map(OsStr::to_owned)
            .unwrap_or_default();
        suffix.push(".tmp");
        let file = Builder::new()
            .prefix(self.prefix)
            .suffix(&suffix)
            .permissions(Permissions::from_mode(self.mode))
            .tempfile_in(self.dir)
            .with_context(|| format!("creating a temporary file in {}", self.dir.display()))?;
        Ok(file.into_temp_path())
    }
}

/// Moves the temporary file over `dest`.
pub fn replace(temp: TempPath, dest: &Path) -> Result<()> {
    temp.persist(dest)
        .with_context(|| format!("writing {}", dest.display()))?;
    Ok(())
}
