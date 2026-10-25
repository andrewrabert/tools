mod cli;
mod config;
mod suffix;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::mime::{self, MimeType};
use crate::tools::Tool;
use crate::tools::find_file_exts::walk;
pub use crate::tools::mime_ext::cli::MimeExt;
use crate::tools::mime_ext::config::{Config, Mode};
use crate::tools::mime_ext::suffix::{Expected, Fix};

type ByType = BTreeMap<MimeType, Vec<PathBuf>>;

enum Outcome {
    Clean,
    /// Some file has the wrong suffix and still does.
    Wrong,
}

impl Tool for MimeExt {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(Outcome::Clean) => ExitCode::SUCCESS,
            Ok(Outcome::Wrong) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: MimeExt) -> Result<Outcome> {
    let config = Config::from(args);
    let files: Vec<PathBuf> = walk::files_under(&config.roots)?.into_iter().collect();
    let mut by_type = ByType::new();
    for path in files {
        let mime = mime::detect(&path)?;
        by_type.entry(mime).or_default().push(path);
    }

    match config.mode {
        Mode::List => {
            print_json(&by_type)?;
            Ok(Outcome::Clean)
        }
        Mode::Check => {
            let wrong = check(&by_type);
            print_json(&wrong)?;
            Ok(if wrong.is_empty() {
                Outcome::Clean
            } else {
                Outcome::Wrong
            })
        }
        Mode::Fix { force } => fix(&by_type, force),
    }
}

fn print_json(by_type: &ByType) -> Result<()> {
    let object: Map<String, Value> = by_type
        .iter()
        .map(|(mime, paths)| {
            let paths = paths
                .iter()
                .map(|path| Value::from(path.to_string_lossy()))
                .collect();
            (mime.to_string(), Value::Array(paths))
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&object)?);
    Ok(())
}

/// The files whose suffix does not fit their type, grouped like the input.
fn check(by_type: &ByType) -> ByType {
    let mut wrong = ByType::new();
    for (mime, paths) in by_type {
        let expected = Expected::of(mime);
        warn_unrecognized(&expected, mime);
        for path in paths {
            if !matches!(expected.fix(&suffix::of(path), false), Fix::Keep) {
                wrong.entry(mime.clone()).or_default().push(path.clone());
            }
        }
    }
    wrong
}

fn fix(by_type: &ByType, force: bool) -> Result<Outcome> {
    let mut outcome = Outcome::Clean;
    for (mime, paths) in by_type {
        let expected = Expected::of(mime);
        if !force {
            warn_unrecognized(&expected, mime);
        }
        for path in paths {
            match expected.fix(&suffix::of(path), force) {
                Fix::Keep => {}
                Fix::Append(suffix) => rename(path, suffix)?,
                Fix::Unfixable => {
                    eprintln!(
                        "warning: {mime} under a suffix of another type: {}",
                        path.display()
                    );
                    outcome = Outcome::Wrong;
                }
            }
        }
    }
    Ok(outcome)
}

fn warn_unrecognized(expected: &Expected, mime: &MimeType) {
    if matches!(expected, Expected::Unrecognized) {
        eprintln!("warning: suffix unknown for \"{mime}\"");
    }
}

/// Appends `suffix` to the file's name, padding with underscores past any name already taken.
fn rename(path: &Path, suffix: &str) -> Result<()> {
    let name = path.file_name().unwrap_or_default();
    let target = (0..)
        .map(|underscores| {
            let mut new_name = name.to_os_string();
            new_name.push("_".repeat(underscores));
            new_name.push(suffix);
            path.with_file_name(new_name)
        })
        .find(|target| fs::symlink_metadata(target).is_err())
        .expect("some padded name is free");
    fs::rename(path, &target).with_context(|| format!("renaming {}", path.display()))?;
    println!("\"{}\" -> \"{}\"", path.display(), target.display());
    Ok(())
}
