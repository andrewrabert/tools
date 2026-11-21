//! Files written to a temporary sibling, then moved over their destination.
use std::ffi::OsStr;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result};
use tempfile::{Builder, TempPath};

const DEFAULT_MODE: u32 = 0o666;

pub fn sibling(prefix: &str, dest: &Path) -> Result<TempPath> {
    let dir = dest
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut suffix = dest.file_name().map(OsStr::to_owned).unwrap_or_default();
    suffix.push(".tmp");
    let file = Builder::new()
        .prefix(prefix)
        .suffix(&suffix)
        .permissions(Permissions::from_mode(DEFAULT_MODE))
        .tempfile_in(dir)
        .with_context(|| format!("creating a temporary file in {}", dir.display()))?;
    Ok(file.into_temp_path())
}

pub fn replace(temp: TempPath, dest: &Path) -> Result<()> {
    temp.persist(dest)
        .with_context(|| format!("writing {}", dest.display()))?;
    Ok(())
}
