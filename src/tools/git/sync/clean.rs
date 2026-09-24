use std::fs;
use std::path::{Path, PathBuf};

pub fn empty_dirs(root: &Path, dry_run: bool) -> usize {
    let mut deleted = 0;
    clean(root, true, dry_run, &mut deleted);
    deleted
}

fn clean(dir: &Path, is_root: bool, dry_run: bool, deleted: &mut usize) {
    if dir.join(".git").exists() {
        return;
    }
    let Ok(children) = children(dir) else {
        return;
    };
    if !is_root && children.iter().any(|(_, is_dir)| !is_dir) {
        return;
    }
    for (child, is_dir) in &children {
        if *is_dir {
            clean(child, false, dry_run, deleted);
        }
    }
    if is_root || !fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none()) {
        return;
    }
    println!("Deleting {}", dir.display());
    if dry_run || fs::remove_dir(dir).is_ok() {
        *deleted += 1;
    }
}

fn children(dir: &Path) -> std::io::Result<Vec<(PathBuf, bool)>> {
    fs::read_dir(dir)?
        .map(|entry| {
            let entry = entry?;
            let is_dir = entry.file_type()?.is_dir();
            Ok((entry.path(), is_dir))
        })
        .collect()
}
