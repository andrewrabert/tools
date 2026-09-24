use std::ffi::OsStr;
use std::path::Path;

use anyhow::{Context, Result};
use tempfile::{Builder, TempDir, TempPath};

const SUFFIX: &str = ".tmp";

pub fn dir(parent: &Path, name: &OsStr, prefix: &str) -> Result<TempDir> {
    let mut suffix = name.to_owned();
    suffix.push(SUFFIX);
    Builder::new()
        .prefix(prefix)
        .suffix(&suffix)
        .tempdir_in(parent)
        .with_context(|| format!("creating a temporary directory in {}", parent.display()))
}

pub fn file(parent: &Path, name: &OsStr, prefix: &str) -> Result<TempPath> {
    let mut suffix = name.to_owned();
    suffix.push(SUFFIX);
    let file = Builder::new()
        .prefix(prefix)
        .suffix(&suffix)
        .tempfile_in(parent)
        .with_context(|| format!("creating a temporary file in {}", parent.display()))?;
    Ok(file.into_temp_path())
}
