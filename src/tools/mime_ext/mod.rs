mod cli;
mod config;
mod detect;
mod suffix;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use serde_json::{Map, Value};

use crate::tools::Tool;
use crate::tools::find_file_exts::walk;
pub use crate::tools::mime_ext::cli::MimeExt;
use crate::tools::mime_ext::config::{Config, Mode};
use crate::tools::mime_ext::suffix::{Kind, Verdict};

type ByType = BTreeMap<String, Vec<PathBuf>>;

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
    let types = detect::mime_types(&files)?;
    let mut by_type = ByType::new();
    for (path, mime) in files.into_iter().zip(types) {
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
        Mode::Fix { force } => Ok(fix(&by_type, force)),
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
            (mime.clone(), Value::Array(paths))
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&object)?);
    Ok(())
}

/// The files whose suffix does not fit their type, grouped like the input.
fn check(by_type: &ByType) -> ByType {
    let mut wrong = ByType::new();
    for (mime, paths) in by_type {
        let kind = Kind::of(mime);
        warn_unknown(&kind, mime);
        for path in paths {
            if !matches!(kind.judge(&suffix::of(path), false), Verdict::Fits) {
                wrong.entry(mime.clone()).or_default().push(path.clone());
            }
        }
    }
    wrong
}

fn fix(by_type: &ByType, force: bool) -> Outcome {
    let mut outcome = Outcome::Clean;
    for (mime, paths) in by_type {
        let kind = Kind::of(mime);
        if !force {
            warn_unknown(&kind, mime);
        }
        for path in paths {
            match kind.judge(&suffix::of(path), force) {
                Verdict::Fits => {}
                Verdict::Wrong(suffix) => {
                    if !rename(path, suffix) {
                        outcome = Outcome::Wrong;
                    }
                }
                Verdict::Unfixable => {
                    eprintln!(
                        "warning: {mime} under a suffix of another type: {}",
                        path.display()
                    );
                    outcome = Outcome::Wrong;
                }
            }
        }
    }
    outcome
}

fn warn_unknown(kind: &Kind, mime: &str) {
    if matches!(kind, Kind::Unknown) {
        eprintln!("warning: suffix unknown for \"{mime}\"");
    }
}

/// Appends `suffix` to the file's name, padding with underscores past any name already taken.
fn rename(path: &Path, suffix: &str) -> bool {
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
    match fs::rename(path, &target) {
        Ok(()) => {
            println!("\"{}\" -> \"{}\"", path.display(), target.display());
            true
        }
        Err(error) => {
            eprintln!("error: rename failed for \"{}\": {error}", path.display());
            false
        }
    }
}
