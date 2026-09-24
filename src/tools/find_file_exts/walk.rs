use std::collections::BTreeSet;
use std::fs;
use std::path::{self, PathBuf};

use anyhow::{Context, Result};

pub fn files_under(roots: &[PathBuf]) -> Result<BTreeSet<PathBuf>> {
    let mut files = BTreeSet::new();
    let mut stack = Vec::new();
    for root in roots {
        let root = path::absolute(root).with_context(|| format!("resolving {}", root.display()))?;
        fs::metadata(&root).with_context(|| format!("stat {}", root.display()))?;
        sort(root, &mut files, &mut stack);
    }
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
            sort(entry.path(), &mut files, &mut stack);
        }
    }
    Ok(files)
}

fn sort(path: PathBuf, files: &mut BTreeSet<PathBuf>, dirs: &mut Vec<PathBuf>) {
    if path.is_file() {
        files.insert(path);
    } else if !path.is_symlink() && path.is_dir() {
        dirs.push(path);
    }
}
