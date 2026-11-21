use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, OpenOptions, Permissions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::atomic_write;
use crate::tools::bertbox::packages::paths;

const EXECUTABLE: u32 = 0o755;

pub fn write(dest: &Path, data: &[u8]) -> Result<()> {
    write_with_mode(dest, data, None)
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

pub fn write_binary(name: &str, data: &[u8]) -> Result<PathBuf> {
    write_executable(name, name, data)
}

pub fn write_executable(name: &str, file_name: &str, data: &[u8]) -> Result<PathBuf> {
    let dest = paths::package_bin(name)?.join(file_name);
    write_with_mode(&dest, data, Some(EXECUTABLE))?;
    println!("Installed: {}", dest.display());
    Ok(dest)
}

pub fn has_executables(name: &str) -> Result<bool> {
    let bin = paths::package_bin(name)?;
    match fs::read_dir(&bin) {
        Ok(mut entries) => Ok(entries.next().is_some()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("reading {}", bin.display())),
    }
}

pub fn remove_other_executables(name: &str, keep: &HashSet<OsString>) -> Result<()> {
    let bin = paths::package_bin(name)?;
    for entry in fs::read_dir(&bin).with_context(|| format!("reading {}", bin.display()))? {
        let entry = entry.with_context(|| format!("reading {}", bin.display()))?;
        if !keep.contains(&entry.file_name()) {
            fs::remove_file(entry.path())
                .with_context(|| format!("removing {}", entry.path().display()))?;
        }
    }
    Ok(())
}

pub fn link_package(name: &str) -> Result<()> {
    let bin_dir = paths::bin_dir()?;
    fs::create_dir_all(&bin_dir).with_context(|| format!("creating {}", bin_dir.display()))?;
    let package_bin = paths::package_bin(name)?;
    let entries =
        fs::read_dir(&package_bin).with_context(|| format!("reading {}", package_bin.display()))?;
    for entry in entries {
        let target = entry
            .with_context(|| format!("reading {}", package_bin.display()))?
            .path();
        let Some(file_name) = target.file_name() else {
            continue;
        };
        let link = bin_dir.join(file_name);
        if fs::read_link(&link).is_ok_and(|current| current == target) {
            continue;
        }
        if link.symlink_metadata().is_ok() {
            fs::remove_file(&link).with_context(|| format!("removing {}", link.display()))?;
        }
        symlink(&target, &link)
            .with_context(|| format!("linking {} to {}", link.display(), target.display()))?;
    }
    let package_dir = paths::package_dir(name)?;
    unlink_bin_entries(&package_dir, |target| !target.exists())
}

pub fn remove_package(name: &str) -> Result<bool> {
    let package_dir = paths::package_dir(name)?;
    if !package_dir.exists() {
        return Ok(false);
    }
    unlink_bin_entries(&package_dir, |_| true)?;
    fs::remove_dir_all(&package_dir)
        .with_context(|| format!("removing {}", package_dir.display()))?;
    println!("Removed: {}", package_dir.display());
    Ok(true)
}

fn unlink_bin_entries(package_dir: &Path, remove: impl Fn(&Path) -> bool) -> Result<()> {
    let bin_dir = paths::bin_dir()?;
    let entries = match fs::read_dir(&bin_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", bin_dir.display())),
    };
    for entry in entries {
        let path = entry
            .with_context(|| format!("reading {}", bin_dir.display()))?
            .path();
        let Ok(target) = fs::read_link(&path) else {
            continue;
        };
        let target = bin_dir.join(target);
        if target.starts_with(package_dir) && remove(&target) {
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
    }
    Ok(())
}
