use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::walk;

/// Every file under the given paths, sorted; directories are entered through symlinks.
pub fn all_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();
    for path in paths {
        if !path.exists() {
            bail!("path does not exist: {}", path.display());
        }
        for entry in walk::entries(path) {
            let entry = entry?;
            if !entry.file_type().is_dir() {
                files.insert(entry.into_path());
            }
        }
    }
    Ok(files.into_iter().collect())
}
