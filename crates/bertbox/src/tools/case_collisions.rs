use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Args as ClapArgs;

use crate::tools::Tool;
use crate::walk;

#[derive(ClapArgs)]
#[command(
    about = "List paths under a directory that would collide on a case-insensitive filesystem"
)]
pub struct CaseCollisions {
    #[arg(value_name = "PATH", help = "directory to search recursively")]
    path: PathBuf,
}

impl Tool for CaseCollisions {
    fn run(self) -> ExitCode {
        match run(&self.path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

/// Exits nonzero when any collision is found, so the result can drive scripts.
fn run(root: &Path) -> Result<()> {
    if !root.exists() {
        bail!("path does not exist: {}", root.display());
    }
    let collisions = collisions_under(root)?;
    println!("{}", serde_json::to_string_pretty(&collisions)?);
    if !collisions.is_empty() {
        bail!("{} colliding groups found", collisions.len());
    }
    Ok(())
}

/// Paths under `root` that share a lowercased path, keyed by that lowercased path.
fn collisions_under(root: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    let mut by_lowercase: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in walk::entries(root) {
        let entry = entry?;
        if entry.depth() == 0 {
            continue;
        }
        let path = entry.path().to_string_lossy().into_owned();
        by_lowercase
            .entry(path.to_lowercase())
            .or_default()
            .push(path);
    }
    by_lowercase.retain(|_, paths| paths.len() > 1);
    for paths in by_lowercase.values_mut() {
        paths.sort();
    }
    Ok(by_lowercase)
}
