use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub fn files_under(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let root_meta = fs::metadata(root).with_context(|| format!("stat {}", root.display()))?;
    if !root_meta.is_dir() {
        files.push(root.to_path_buf());
        return Ok(files);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("stat {}", path.display()))?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                files.push(path);
            } else if file_type.is_symlink() && fs::metadata(&path).is_ok_and(|m| m.is_dir()) {
                stack.push(path);
            }
        }
    }
    Ok(files)
}
