use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn empty_dirs(root: &Path, include_root: bool) -> Vec<PathBuf> {
    let mut empties = Vec::new();
    let has_content = scan(root, &mut empties);
    if include_root && !has_content {
        empties.push(root.to_path_buf());
    }
    empties
}

fn scan(dir: &Path, empties: &mut Vec<PathBuf>) -> bool {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            if e.kind() != io::ErrorKind::NotFound {
                eprintln!("{}: {e}", dir.display());
            }
            return true;
        }
    };
    let mut has_content = false;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                eprintln!("{}: {e}", dir.display());
                has_content = true;
                continue;
            }
        };
        let path = entry.path();
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if !is_dir || scan(&path, empties) {
            has_content = true;
        } else {
            empties.push(path);
        }
    }
    has_content
}
