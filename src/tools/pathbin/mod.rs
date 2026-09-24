mod cli;
mod config;

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result};
use nix::unistd::{self, AccessFlags};

use crate::tools::Tool;
pub use crate::tools::pathbin::cli::Pathbin;
use crate::tools::pathbin::config::{Config, Grouping};

struct Executable {
    dir: String,
    name: String,
}

impl Tool for Pathbin {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: Pathbin) -> Result<()> {
    let config = Config::from(args);
    let dirs = if config.dirs.is_empty() {
        env::var_os("PATH").map_or_else(Vec::new, |path| env::split_paths(&path).collect())
    } else {
        config.dirs
    };

    let mut executables = Vec::new();
    for dir in dirs.iter().filter(|dir| dir.is_dir()) {
        executables.extend(executables_in(dir)?);
    }

    match config.grouping {
        Grouping::Flat => {
            let names: BTreeSet<&str> = executables.iter().map(|e| e.name.as_str()).collect();
            for name in names {
                println!("{name}");
            }
        }
        Grouping::ByCommand => {
            let mut dirs_by_command: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for executable in &executables {
                dirs_by_command
                    .entry(&executable.name)
                    .or_default()
                    .push(&executable.dir);
            }
            println!("{}", serde_json::to_string_pretty(&dirs_by_command)?);
        }
        Grouping::ByDirectory => {
            let mut commands_by_dir: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for executable in &executables {
                commands_by_dir
                    .entry(&executable.dir)
                    .or_default()
                    .push(&executable.name);
            }
            for commands in commands_by_dir.values_mut() {
                commands.sort();
            }
            println!("{}", serde_json::to_string_pretty(&commands_by_dir)?);
        }
    }
    Ok(())
}

fn executables_in(dir: &Path) -> Result<Vec<Executable>> {
    let entries = fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    let mut executables = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        if is_executable_file(&entry.path()) {
            executables.push(Executable {
                dir: dir.to_string_lossy().into_owned(),
                name: entry.file_name().to_string_lossy().into_owned(),
            });
        }
    }
    Ok(executables)
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file() && unistd::access(path, AccessFlags::X_OK).is_ok()
}
