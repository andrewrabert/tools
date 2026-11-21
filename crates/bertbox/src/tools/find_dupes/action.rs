use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::tools::find_dupes::config::Action;
use crate::tools::find_dupes::dupes::{DuplicateSets, Sha256Hex};

const COLLISION_SUFFIX: &str = ".~1~";

pub fn run(action: &Action, sets: DuplicateSets) -> Result<()> {
    match action {
        Action::Report => {
            let report: BTreeMap<&str, Vec<String>> = sets
                .duplicates
                .iter()
                .map(|(hash, paths)| (hash.as_str(), paths.iter().map(|p| display(p)).collect()))
                .collect();
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Action::Unique => {
            let list: Vec<String> = sets.unique.iter().map(|p| display(p)).collect();
            println!("{}", serde_json::to_string_pretty(&list)?);
        }
        Action::Remove { dry_run } => {
            for paths in sets.duplicates.values() {
                for path in &paths[1..] {
                    println!("{}", path.display());
                    if !dry_run {
                        fs::remove_file(path)
                            .with_context(|| format!("removing {}", path.display()))?;
                    }
                }
            }
        }
        Action::Move(dir) => move_all(dir, &sets.duplicates)?,
    }
    Ok(())
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn move_all(dir: &Path, duplicates: &BTreeMap<Sha256Hex, Vec<PathBuf>>) -> Result<()> {
    fs::create_dir(dir).with_context(|| format!("creating {}", dir.display()))?;
    for (hash, paths) in duplicates {
        let hash_dir = dir.join(hash.as_str());
        fs::create_dir(&hash_dir).with_context(|| format!("creating {}", hash_dir.display()))?;
        for path in paths {
            let mut name = path
                .file_name()
                .with_context(|| format!("no file name: {}", path.display()))?
                .to_os_string();
            while hash_dir.join(&name).exists() {
                name.push(COLLISION_SUFFIX);
            }
            let dest = hash_dir.join(&name);
            move_file(path, &dest)
                .with_context(|| format!("moving {} -> {}", path.display(), dest.display()))?;
        }
    }
    Ok(())
}

fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            fs::copy(from, to)?;
            fs::remove_file(from)
        }
        other => other,
    }
}
