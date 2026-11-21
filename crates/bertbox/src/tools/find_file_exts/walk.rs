use std::collections::BTreeSet;
use std::fs;
use std::path::{self, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

/// Files under the roots, made absolute; symlinked directories are not entered.
pub fn files_under(roots: &[PathBuf]) -> Result<BTreeSet<PathBuf>> {
    let mut files = BTreeSet::new();
    for root in roots {
        let root = path::absolute(root).with_context(|| format!("resolving {}", root.display()))?;
        fs::metadata(&root).with_context(|| format!("stat {}", root.display()))?;
        for entry in WalkDir::new(&root).follow_root_links(false) {
            let entry = entry?;
            if entry.path().is_file() {
                files.insert(entry.into_path());
            }
        }
    }
    Ok(files)
}
