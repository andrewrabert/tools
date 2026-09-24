use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::walk;

pub fn files_under(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in walk::entries(root) {
        let entry = entry?;
        if entry.file_type().is_file() {
            files.push(entry.into_path());
        }
    }
    files.sort();
    Ok(files)
}
