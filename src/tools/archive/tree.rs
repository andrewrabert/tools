use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub fn paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = vec![root.to_owned()];
    let mut stack = Vec::new();
    let metadata =
        fs::symlink_metadata(root).with_context(|| format!("reading {}", root.display()))?;
    if metadata.is_dir() {
        stack.push(root.to_owned());
    }
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("listing {}", dir.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("reading {}", path.display()))?;
            if file_type.is_dir() {
                stack.push(path.clone());
            }
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}
