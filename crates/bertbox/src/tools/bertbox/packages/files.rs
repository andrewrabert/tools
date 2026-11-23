use std::fs::{self, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result};

use crate::atomic_write;

const EXECUTABLE: u32 = 0o755;

pub fn write(dest: &Path, data: &[u8]) -> Result<()> {
    write_with_mode(dest, data, None)
}

pub fn write_executable(dest: &Path, data: &[u8]) -> Result<()> {
    write_with_mode(dest, data, Some(EXECUTABLE))
}

fn write_with_mode(dest: &Path, data: &[u8], mode: Option<u32>) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let temp = atomic_write::sibling(".", dest)?;
    let mut file = OpenOptions::new()
        .write(true)
        .open(&temp)
        .with_context(|| format!("opening {}", temp.display()))?;
    file.write_all(data)
        .and_then(|()| file.sync_all())
        .with_context(|| format!("writing {}", temp.display()))?;
    if let Some(mode) = mode {
        fs::set_permissions(&temp, Permissions::from_mode(mode))
            .with_context(|| format!("setting the mode of {}", temp.display()))?;
    }
    atomic_write::replace(temp, dest)
}
