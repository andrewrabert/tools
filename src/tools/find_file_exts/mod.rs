mod cli;
mod config;
mod suffix;
pub mod walk;

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::tools::Tool;
pub use crate::tools::find_file_exts::cli::FindFileExts;
use crate::tools::find_file_exts::config::{Config, Output};
use crate::tools::find_file_exts::suffix::Suffix;

impl Tool for FindFileExts {
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

fn run(args: FindFileExts) -> Result<()> {
    let config = Config::from(args);
    let files = if io::stdin().is_terminal() {
        walked(config.roots)?
    } else {
        listed()?
    };

    let mut by_suffix: BTreeMap<Suffix, BTreeSet<PathBuf>> = BTreeMap::new();
    for path in files {
        by_suffix
            .entry(Suffix::of(&path, &config.folding))
            .or_default()
            .insert(path);
    }

    match config.output {
        Output::Suffixes => {
            for suffix in by_suffix.keys() {
                println!("{suffix}");
            }
        }
        Output::Counts => {
            let mut counts: Vec<(usize, &Suffix)> = by_suffix
                .iter()
                .map(|(suffix, paths)| (paths.len(), suffix))
                .collect();
            counts.sort();
            for (count, suffix) in counts {
                println!("{suffix}\t\t{count}");
            }
        }
        Output::Json => {
            let object: Map<String, Value> = by_suffix
                .iter()
                .map(|(suffix, paths)| {
                    let paths = paths
                        .iter()
                        .map(|path| Value::from(path.to_string_lossy()))
                        .collect();
                    (suffix.as_str().to_owned(), Value::Array(paths))
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&object)?);
        }
    }
    Ok(())
}

fn walked(mut roots: Vec<PathBuf>) -> Result<BTreeSet<PathBuf>> {
    if roots.is_empty() {
        roots.push(env::current_dir().context("resolving the current directory")?);
    }
    walk::files_under(&roots)
}

fn listed() -> Result<BTreeSet<PathBuf>> {
    let mut files = BTreeSet::new();
    for line in io::stdin().lines() {
        let line = line.context("reading stdin")?;
        if !line.is_empty() && !line.ends_with('/') {
            files.insert(PathBuf::from(line));
        }
    }
    Ok(files)
}
