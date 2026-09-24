use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::walk;

/// Regular files under `root`; symlinks to files are left out.
pub fn files_under(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in walk::entries(root) {
        let entry = entry?;
        if entry.file_type().is_file() && !entry.path_is_symlink() {
            files.push(entry.into_path());
        }
    }
    Ok(files)
}
