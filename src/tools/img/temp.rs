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

pub fn with_suffix(dir: &Path, prefix: &str, suffix: &str) -> Result<TempPath> {
    file(dir, prefix, OsStr::new(suffix))
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
