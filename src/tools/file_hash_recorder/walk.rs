use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub fn files_under(root: &Path) -> Result<Vec<PathBuf>> {
    if root.is_file() {
        return Ok(vec![root.to_path_buf()]);
    }
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;
        for entry in entries {
            let path = entry
                .with_context(|| format!("reading {}", dir.display()))?
                .path();
            if path.is_file() {
                files.push(path);
            } else if path.is_dir() {
                stack.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}
