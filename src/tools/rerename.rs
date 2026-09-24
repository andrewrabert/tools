use std::ffi::{OsStr, OsString};
use std::fs::{self, Metadata};
use std::io::{self, Read};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args as ClapArgs, ValueEnum};
use regex::bytes::{Captures, Regex, RegexBuilder};

use crate::tools::Tool;
use crate::walk;

#[derive(ClapArgs)]
#[command(about = "Rename files using regex substitution")]
pub struct Rerename {
    #[arg(short, long, help = "overwrite target files if they exist")]
    force: bool,
    #[arg(
        short = 'n',
        long,
        help = "show what would be renamed without doing it"
    )]
    dry_run: bool,
    #[arg(short, long, help = "recursively process directories")]
    recursive: bool,
    #[arg(short, long, help = "case-insensitive pattern matching")]
    ignore_case: bool,
    #[arg(
        short = 't',
        long = "type",
        value_name = "TYPE",
        value_enum,
        default_values_t = [Kind::File, Kind::Dir],
        help = "types to process (can be specified multiple times)"
    )]
    kinds: Vec<Kind>,
    #[arg(
        short = '0',
        long,
        help = "paths from stdin are null-delimited instead of newline-delimited"
    )]
    null: bool,
    #[command(flatten)]
    operation: Operation,
    #[arg(value_name = "PATTERN", help = "regex pattern to match")]
    pattern: String,
    #[arg(value_name = "PATH", help = "paths to rename (or read from stdin)")]
    paths: Vec<PathBuf>,
}

#[derive(ClapArgs)]
#[group(required = true, multiple = false)]
struct Operation {
    #[arg(long, value_name = "STRING", help = "replacement string")]
    replace: Option<OsString>,
    #[arg(long, help = "lowercase the matched text")]
    lower: bool,
    #[arg(long, help = "uppercase the matched text")]
    upper: bool,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum Kind {
    File,
    Dir,
}

impl Kind {
    fn of(path: &Path) -> Option<Kind> {
        if path.is_dir() {
            Some(Kind::Dir)
        } else if path.is_file() {
            Some(Kind::File)
        } else {
            None
        }
    }
}

enum Transform {
    Replace(Vec<u8>),
    Lower,
    Upper,
}

impl From<Operation> for Transform {
    fn from(operation: Operation) -> Self {
        match operation {
            Operation {
                replace: Some(replacement),
                ..
            } => Transform::Replace(replacement.into_vec()),
            Operation { lower: true, .. } => Transform::Lower,
            Operation { .. } => Transform::Upper,
        }
    }
}

impl Transform {
    fn apply(&self, regex: &Regex, name: &OsStr) -> OsString {
        let name = name.as_bytes();
        let renamed = match self {
            Transform::Replace(replacement) => regex.replace_all(name, replacement.as_slice()),
            Transform::Lower => regex.replace_all(name, |found: &Captures| lower(&found[0])),
            Transform::Upper => regex.replace_all(name, |found: &Captures| upper(&found[0])),
        };
        OsString::from_vec(renamed.into_owned())
    }
}

struct TargetExists;

enum Plan {
    Direct,
    Overwrite,
    ViaTemp,
}

impl Plan {
    fn resolve(path: &Path, new_path: &Path, force: bool) -> Result<Plan, TargetExists> {
        let Ok(target) = fs::symlink_metadata(new_path) else {
            return Ok(Plan::Direct);
        };
        let same_file = fs::symlink_metadata(path).is_ok_and(|source| same_file(&source, &target));
        if same_file {
            Ok(Plan::ViaTemp)
        } else if force {
            Ok(Plan::Overwrite)
        } else {
            Err(TargetExists)
        }
    }

    fn execute(&self, path: &Path, new_path: &Path) -> Result<()> {
        match self {
            Plan::Direct => fs::rename(path, new_path)?,
            Plan::Overwrite => {
                fs::remove_file(new_path)?;
                fs::rename(path, new_path)?;
            }
            Plan::ViaTemp => {
                let dir = path.parent().unwrap_or(Path::new(""));
                let reserved = tempfile::Builder::new().prefix(".tmp_").tempfile_in(dir)?;
                let temp = reserved.path().to_path_buf();
                reserved.close()?;
                fs::rename(path, &temp)?;
                if let Err(error) = fs::rename(&temp, new_path) {
                    fs::rename(&temp, path)
                        .with_context(|| format!("original left at {}", temp.display()))?;
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }
}

impl Tool for Rerename {
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

fn run(args: Rerename) -> Result<()> {
    let regex = RegexBuilder::new(&args.pattern)
        .case_insensitive(args.ignore_case)
        .build()?;
    let transform = Transform::from(args.operation);
    let roots = if args.paths.is_empty() {
        stdin_paths(args.null)?
    } else {
        args.paths
    };

    for path in targets(roots, args.recursive, &args.kinds)? {
        let Some(name) = path.file_name() else {
            continue;
        };
        let new_name = transform.apply(&regex, name);
        if new_name == name {
            continue;
        }
        let new_path = path.with_file_name(&new_name);
        let name = Path::new(name).display();
        let new_name = Path::new(&new_name).display();

        let plan = match Plan::resolve(&path, &new_path, args.force) {
            Ok(plan) => plan,
            Err(TargetExists) => {
                eprintln!("error: target exists \"{new_name}\" (original \"{name}\")");
                continue;
            }
        };
        if args.dry_run {
            println!("\"{}\" -> \"{}\"", path.display(), new_path.display());
            continue;
        }
        match plan.execute(&path, &new_path) {
            Ok(()) => println!("{name}\n{new_name}\n"),
            Err(error) => eprintln!("error: rename failed for \"{name}\": {error:#}"),
        }
    }
    Ok(())
}

fn stdin_paths(null: bool) -> io::Result<Vec<PathBuf>> {
    let mut bytes = Vec::new();
    io::stdin().lock().read_to_end(&mut bytes)?;
    let delimiter = if null { b'\0' } else { b'\n' };
    Ok(bytes
        .split(|byte| *byte == delimiter)
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(OsStr::from_bytes(path)))
        .collect())
}

fn targets(roots: Vec<PathBuf>, recursive: bool, kinds: &[Kind]) -> Result<Vec<PathBuf>> {
    let mut targets = Vec::new();
    for root in roots {
        if recursive && !root.exists() {
            bail!("path does not exist: {}", root.display());
        }
        let Some(kind) = Kind::of(&root) else {
            continue;
        };
        if !recursive || kind == Kind::File {
            if kinds.contains(&kind) {
                targets.push(root);
            }
            continue;
        }
        for entry in walk::entries(&root) {
            let entry = entry?;
            let kind = if entry.file_type().is_dir() {
                Kind::Dir
            } else {
                Kind::File
            };
            if kinds.contains(&kind) {
                targets.push(entry.into_path());
            }
        }
    }

    // Deepest paths first, so children are renamed before their parents move.
    targets.sort_by(|a, b| depth(b).cmp(&depth(a)).then_with(|| a.cmp(b)));
    targets.dedup();
    Ok(targets)
}

fn depth(path: &Path) -> usize {
    path.components().count()
}

fn same_file(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

fn lower(bytes: &[u8]) -> Vec<u8> {
    match str::from_utf8(bytes) {
        Ok(text) => text.to_lowercase().into_bytes(),
        Err(_) => bytes.to_ascii_lowercase(),
    }
}

fn upper(bytes: &[u8]) -> Vec<u8> {
    match str::from_utf8(bytes) {
        Ok(text) => text.to_uppercase().into_bytes(),
        Err(_) => bytes.to_ascii_uppercase(),
    }
}
