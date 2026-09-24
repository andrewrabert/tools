//! Syncthing names a version by its original name plus a `~YYYYMMDD-HHMMSS` stamp, placed
//! before the last extension. For a version stamped 20250921-021118:
//!
//! - `thing` -> `thing~20250921-021118`
//! - `.DS_Store` -> `~20250921-021118.DS_Store`
//! - `stuff~20250918-223004.json` -> `stuff~20250918-223004~20250921-021118.json`
//! - `package.tar.gz` -> `package.tar~20250921-021118.gz`

use std::collections::BTreeMap;
use std::fs::{self, File, FileTimes};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use clap::{Args as ClapArgs, Subcommand};
use jiff::civil::DateTime;
use regex::Regex;

use crate::tools::Tool;
use crate::walk;

const STVERSIONS: &str = ".stversions";

static STAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"~(\d{8}-\d{6})(\..+)?$").expect("valid regex"));

#[derive(ClapArgs)]
#[command(about = "List, prune, and restore Syncthing's .stversions files")]
pub struct SyncthingStversions {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List versions grouped by original path, as JSON
    List(Filter),
    /// Remove every version but the latest of each original path
    RmKeepLatest(Filter),
    /// Restore the newest version of each original path to a directory
    RestoreNewest(Restore),
    /// Restore the oldest version of each original path to a directory
    RestoreOldest(Restore),
}

#[derive(ClapArgs)]
struct Filter {
    /// Path to the synced folder
    #[arg(value_name = "PATH")]
    path: PathBuf,
    /// Only versions stamped before this datetime (ISO 8601)
    #[arg(long, value_name = "DATETIME")]
    before: Option<DateTime>,
    /// Only versions stamped after this datetime (ISO 8601)
    #[arg(long, value_name = "DATETIME")]
    after: Option<DateTime>,
}

#[derive(ClapArgs)]
struct Restore {
    #[command(flatten)]
    filter: Filter,
    /// Directory to restore files into
    #[arg(short, long, value_name = "DIR")]
    output: PathBuf,
    /// Overwrite existing files
    #[arg(short, long)]
    force: bool,
}

impl Tool for SyncthingStversions {
    fn run(self) -> ExitCode {
        match run(self.command) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

/// Versions keyed by original path, each keyed by version path. Both relative to `.stversions`.
type Versions = BTreeMap<PathBuf, BTreeMap<PathBuf, DateTime>>;

fn run(command: Command) -> Result<()> {
    match command {
        Command::List(filter) => {
            let root = filter.path.join(STVERSIONS);
            let versions = versions(&root, &filter)?;
            print_json(&root, &versions)
        }
        Command::RmKeepLatest(filter) => {
            let root = filter.path.join(STVERSIONS);
            let mut versions = versions(&root, &filter)?;
            for by_path in versions.values_mut() {
                let mut by_date: Vec<(PathBuf, DateTime)> = by_path
                    .iter()
                    .map(|(path, date)| (path.clone(), *date))
                    .collect();
                by_date.sort_by_key(|(_, date)| *date);
                by_date.pop();
                for (path, _) in by_date {
                    let full = root.join(&path);
                    fs::remove_file(&full)
                        .with_context(|| format!("removing {}", full.display()))?;
                    println!("Deleted: {}", path.display());
                    by_path.remove(&path);
                }
            }
            print_json(&root, &versions)
        }
        Command::RestoreNewest(restore) => restore_each(&restore, |by_path| {
            by_path.iter().max_by_key(|(_, date)| **date)
        }),
        Command::RestoreOldest(restore) => restore_each(&restore, |by_path| {
            by_path.iter().min_by_key(|(_, date)| **date)
        }),
    }
}

fn versions(root: &Path, filter: &Filter) -> Result<Versions> {
    if !root.exists() {
        bail!("path does not exist: {}", root.display());
    }
    let mut versions = Versions::new();
    for entry in walk::entries(root) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .strip_prefix(root)
            .context("walked outside .stversions")?
            .to_path_buf();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(captures) = STAMP.captures(name) else {
            continue;
        };
        let stamp = &captures[1];
        let date = DateTime::strptime("%Y%m%d-%H%M%S", stamp)
            .with_context(|| format!("parsing the version stamp of {}", path.display()))?;
        if filter.before.is_some_and(|before| date >= before)
            || filter.after.is_some_and(|after| date <= after)
        {
            continue;
        }
        let start = captures.get(0).expect("whole match").start();
        let extension = captures.get(2).map_or("", |extension| extension.as_str());
        let original = path.with_file_name(format!("{}{extension}", &name[..start]));
        versions.entry(original).or_default().insert(path, date);
    }
    Ok(versions)
}

fn print_json(root: &Path, versions: &Versions) -> Result<()> {
    let versions: BTreeMap<String, BTreeMap<String, String>> = versions
        .iter()
        .map(|(original, by_path)| {
            let by_path = by_path
                .iter()
                .map(|(path, date)| (path.display().to_string(), date.to_string()))
                .collect();
            (original.display().to_string(), by_path)
        })
        .collect();
    let output = BTreeMap::from([(root.display().to_string(), versions)]);
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn restore_each(
    restore: &Restore,
    pick: impl Fn(&BTreeMap<PathBuf, DateTime>) -> Option<(&PathBuf, &DateTime)>,
) -> Result<()> {
    let root = restore.filter.path.join(STVERSIONS);
    for (original, by_path) in versions(&root, &restore.filter)? {
        let Some((path, _)) = pick(&by_path) else {
            continue;
        };
        let source = root.join(path);
        let dest = restore.output.join(&original);
        if dest.exists() && !restore.force {
            eprintln!(
                "Error: {} already exists. Use -f/--force to overwrite.",
                dest.display()
            );
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        copy_with_times(&source, &dest)?;
        println!("Restored: {} -> {}", source.display(), dest.display());
    }
    Ok(())
}

/// Copies contents, permissions, and access and modification times.
fn copy_with_times(source: &Path, dest: &Path) -> Result<()> {
    let context = || format!("copying {} to {}", source.display(), dest.display());
    fs::copy(source, dest).with_context(context)?;
    let metadata = fs::metadata(source).with_context(context)?;
    let times = FileTimes::new()
        .set_accessed(metadata.accessed().with_context(context)?)
        .set_modified(metadata.modified().with_context(context)?);
    File::options()
        .write(true)
        .open(dest)
        .and_then(|file| file.set_times(times))
        .with_context(context)?;
    Ok(())
}
