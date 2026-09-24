use std::ffi::{OsStr, OsString};
use std::path::Path;

use anyhow::{Context, Result};
use tempfile::{Builder, TempPath};

/// An empty temporary file in `dir`, removed on drop unless replaced away.
pub fn file(dir: &Path, prefix: &str, suffix: &OsStr) -> Result<TempPath> {
    let file = Builder::new()
        .prefix(prefix)
        .suffix(suffix)
        .tempfile_in(dir)
        .with_context(|| format!("creating a temporary file in {}", dir.display()))?;
    Ok(file.into_temp_path())
}

/// A temporary file in `dir` named after `path` plus `.tmp`.
pub fn sibling(dir: &Path, prefix: &str, path: &Path) -> Result<TempPath> {
    let mut suffix = path.file_name().map(OsStr::to_owned).unwrap_or_default();
    suffix.push(".tmp");
    file(dir, prefix, &suffix)
}

pub fn with_suffix(dir: &Path, prefix: &str, suffix: &str) -> Result<TempPath> {
    file(dir, prefix, OsStr::new(suffix))
}

/// Moves the temporary file over `dest`.
pub fn replace(temp: TempPath, dest: &Path) -> Result<()> {
    temp.persist(dest)
        .with_context(|| format!("writing {}", dest.display()))?;
    Ok(())
}

/// The temporary directory for scratch data that no output depends on.
pub fn scratch(prefix: &str, suffix: &str) -> Result<TempPath> {
    let file = Builder::new()
        .prefix(prefix)
        .suffix(suffix)
        .tempfile()
        .context("creating a temporary file")?;
    Ok(file.into_temp_path())
}

pub fn os(text: &str) -> OsString {
    OsString::from(text)
}
