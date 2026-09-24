use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Every file under the given paths, sorted; directories are entered through symlinks.
pub fn all_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();
    let mut stack = Vec::new();
    for path in paths {
        if !path.exists() {
            bail!("path does not exist: {}", path.display());
        } else if path.is_dir() {
            stack.push(path.clone());
        } else {
            files.insert(path.clone());
        }
    }
    while let Some(dir) = stack.pop() {
        for entry in read_dir(&dir)? {
            let path = entry
                .with_context(|| format!("reading {}", dir.display()))?
                .path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.insert(path);
            }
        }
    }
    Ok(files.into_iter().collect())
}

fn read_dir(dir: &Path) -> Result<fs::ReadDir> {
    fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))
}
