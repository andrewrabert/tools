use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

/// Repositories under `root`, relative to it, leaving out any nested inside another.
pub fn repos(root: &Path) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    let mut walk = WalkDir::new(root).sort_by_file_name().into_iter();
    while let Some(entry) = walk.next() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                eprintln!("Error: {error}");
                continue;
            }
        };
        if !entry.file_type().is_dir() {
            continue;
        }
        let is_repo = fs::symlink_metadata(entry.path().join(".git")).is_ok_and(|git| git.is_dir());
        if is_repo {
            walk.skip_current_dir();
            let relative = entry.path().strip_prefix(root).unwrap_or(entry.path());
            repos.push(if relative.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                relative.to_path_buf()
            });
        }
    }
    repos
}
