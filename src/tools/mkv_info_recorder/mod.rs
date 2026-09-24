mod cli;
mod db;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{self, Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use serde::Serialize;

use crate::mkvmerge::{self, Language, TrackType};
use crate::tools::Tool;
pub use crate::tools::mkv_info_recorder::cli::MkvInfoRecorder;
use crate::tools::mkv_info_recorder::cli::{Command, OverrideCommand};
use crate::tools::mkv_info_recorder::db::{FileInfo, Languages, MkvInfoDb};

const DATABASE_NAME: &str = "mkv_info.db";

const NANOSECOND: f64 = 1e-9;

impl Tool for MkvInfoRecorder {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

/// What to do when no database is found.
#[derive(Clone, Copy)]
enum Missing {
    Fail,
    Create,
}

/// Where the database comes from.
struct Database {
    given: Option<PathBuf>,
    missing: Missing,
}

fn run(args: MkvInfoRecorder) -> Result<ExitCode> {
    let database = Database {
        given: args.output,
        missing: if args.mkdb {
            Missing::Create
        } else {
            Missing::Fail
        },
    };
    match args.command {
        Command::Check { paths } => check(&paths, &database),
        Command::List {
            path,
            json,
            absolute,
            languages,
            pattern,
            unexpected,
        } => {
            let filter = ListFilter {
                languages: languages.into_iter().collect(),
                pattern,
                unexpected,
            };
            list(&path, json, absolute, &filter, &database)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Override { command } => {
            match command {
                OverrideCommand::List { path } => override_list(&path, &database)?,
                OverrideCommand::Add { path, languages } => {
                    override_add(&path, languages, &database)?
                }
                OverrideCommand::Rm { path, languages } => {
                    override_rm(&path, languages, &database)?
                }
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn check(paths: &[PathBuf], database: &Database) -> Result<ExitCode> {
    let paths = paths
        .iter()
        .map(|path| absolute(path))
        .collect::<Result<Vec<_>>>()?;
    let db_path = match &database.given {
        Some(path) => absolute(path)?,
        None => {
            let mut found = paths
                .iter()
                .map(|path| locate(path, database.missing))
                .collect::<Result<BTreeSet<_>>>()?;
            if found.len() != 1 {
                let found: Vec<String> = found.iter().map(|p| p.display().to_string()).collect();
                bail!("paths resolve to different databases: {}", found.join(", "));
            }
            found.pop_first().context("no paths")?
        }
    };
    MkvInfoDb::with(&db_path, |db| {
        let paths: Vec<PathBuf> = paths
            .into_iter()
            .map(|path| {
                if path == db_path {
                    db.root().to_path_buf()
                } else {
                    path
                }
            })
            .collect();

        let files = mkvmerge::find_files(&paths)?;
        let mut recorded = BTreeMap::new();
        let mut overrides = BTreeMap::new();
        for path in &paths {
            recorded.extend(db.files_under(path)?);
            overrides.extend(db.overrides_under(path)?);
        }
        let all_overrides = db.overrides_under(db.root())?;

        let progress = ProgressBar::new(files.len() as u64);
        let mut unexpected = Vec::new();
        for path in &files {
            let metadata =
                fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
            let mtime = metadata.mtime() as f64 + NANOSECOND * metadata.mtime_nsec() as f64;
            let size = metadata.size();
            let json = match recorded.get(path) {
                Some(info) if info.size == size && info.mtime == mtime => {
                    info.mkv_info_json.clone()
                }
                _ => {
                    let info = FileInfo {
                        mtime,
                        size,
                        mkv_info_json: mkvmerge::identify_json(path)
                            .with_context(|| format!("identifying {}", path.display()))?,
                    };
                    db.upsert_file(path, &info)?;
                    info.mkv_info_json
                }
            };
            let languages = unexpected_languages(path, &json, &all_overrides)
                .with_context(|| format!("reading the languages of {}", path.display()))?;
            if !languages.is_empty() {
                unexpected.push((path, languages));
            }
            progress.inc(1);
        }
        progress.finish();

        for path in recorded.keys() {
            if files.binary_search(path).is_err() && !path.is_file() {
                db.delete_file(path)?;
            }
        }

        for (path, languages) in unexpected {
            let languages: Vec<&str> = languages.iter().map(String::as_str).collect();
            println!("{} {}", path.display(), languages.join(" "));
        }

        let mut code = ExitCode::SUCCESS;
        for path in overrides.keys() {
            if !path.exists() {
                eprintln!("Error: stale override: {}", path.display());
                code = ExitCode::FAILURE;
            }
        }
        Ok(code)
    })
}

/// The audio and subtitle languages of `path` other than English and those its overrides allow.
fn unexpected_languages(
    path: &Path,
    json: &str,
    overrides: &BTreeMap<PathBuf, Languages>,
) -> Result<Languages> {
    let mut languages = track_languages(json)?;
    languages.remove(Language::English.as_str());
    for allowed in path.ancestors().filter_map(|dir| overrides.get(dir)) {
        languages.retain(|language| !allowed.contains(language));
    }
    Ok(languages)
}

/// The audio and subtitle languages in mkvmerge identify `json`.
fn track_languages(json: &str) -> Result<Languages> {
    let identify = mkvmerge::parse_identify(json)?;
    let mut languages = Languages::new();
    for track in &identify.tracks {
        if matches!(track.kind, TrackType::Audio | TrackType::Subtitles) {
            languages.insert(track.language()?.to_owned());
        }
    }
    Ok(languages)
}

/// Which recorded files `list` prints.
struct ListFilter {
    languages: Languages,
    pattern: Option<String>,
    unexpected: bool,
}

#[derive(Serialize)]
struct ListEntry {
    path: PathBuf,
    mtime: f64,
    size: u64,
    languages: Languages,
    unexpected: Languages,
}

fn list(
    path: &Path,
    json: bool,
    absolute: bool,
    filter: &ListFilter,
    database: &Database,
) -> Result<()> {
    with_db(path, database, |db, path| {
        let overrides = db.overrides_under(db.root())?;
        let mut entries = Vec::new();
        for (path, info) in db.files_under(&path)? {
            let shown = if absolute {
                path.clone()
            } else {
                path.strip_prefix(db.root())?.to_path_buf()
            };
            if let Some(pattern) = &filter.pattern
                && !shown.to_string_lossy().contains(pattern.as_str())
            {
                continue;
            }
            let languages = track_languages(&info.mkv_info_json)
                .with_context(|| format!("reading the languages of {}", path.display()))?;
            if !filter.languages.is_empty() && filter.languages.is_disjoint(&languages) {
                continue;
            }
            let unexpected = unexpected_languages(&path, &info.mkv_info_json, &overrides)
                .with_context(|| format!("reading the languages of {}", path.display()))?;
            if filter.unexpected && unexpected.is_empty() {
                continue;
            }
            entries.push(ListEntry {
                path: shown,
                mtime: info.mtime,
                size: info.size,
                languages,
                unexpected,
            });
        }
        if json {
            println!("{}", serde_json::to_string_pretty(&entries)?);
        } else {
            for entry in &entries {
                println!("{}", entry.path.display());
            }
        }
        Ok(())
    })
}

fn override_list(path: &Path, database: &Database) -> Result<()> {
    with_db(path, database, |db, path| {
        for (path, languages) in db.overrides_under(&path)? {
            let languages: Vec<&str> = languages.iter().map(String::as_str).collect();
            println!("{} {}", path.display(), languages.join(" "));
        }
        Ok(())
    })
}

fn override_add(path: &Path, languages: Vec<String>, database: &Database) -> Result<()> {
    if !path.exists() {
        bail!("path does not exist: {}", path.display());
    }
    with_db(path, database, |db, path| {
        let mut allowed = db.overrides_under(&path)?.remove(&path).unwrap_or_default();
        allowed.extend(languages);
        db.set_override(&path, &allowed)
    })
}

fn override_rm(path: &Path, languages: Vec<String>, database: &Database) -> Result<()> {
    with_db(path, database, |db, path| {
        let Some(mut allowed) = db.overrides_under(&path)?.remove(&path) else {
            bail!("no override for {}", path.display());
        };
        if languages.is_empty() {
            allowed.clear();
        } else {
            for language in &languages {
                allowed.remove(language);
            }
        }
        db.set_override(&path, &allowed)
    })
}

/// Runs `body` on the database for `path`, with `path` made absolute.
fn with_db<R>(
    path: &Path,
    database: &Database,
    body: impl FnOnce(&MkvInfoDb, PathBuf) -> Result<R>,
) -> Result<R> {
    let path = absolute(path)?;
    let db_path = match &database.given {
        Some(given) => absolute(given)?,
        None => locate(&path, database.missing)?,
    };
    MkvInfoDb::with(&db_path, |db| body(db, path))
}

/// The database in `root` or its nearest ancestor, or `root` itself when it is one.
fn locate(root: &Path, missing: Missing) -> Result<PathBuf> {
    let metadata =
        fs::metadata(root).with_context(|| format!("path does not exist: {}", root.display()))?;
    if metadata.is_file() && root.file_name().is_some_and(|name| name == DATABASE_NAME) {
        return Ok(root.to_path_buf());
    }

    let start = if metadata.is_dir() {
        root
    } else {
        root.parent().unwrap_or(root)
    };
    for dir in start.ancestors() {
        let candidate = dir.join(DATABASE_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    match missing {
        Missing::Fail => bail!(
            "no {DATABASE_NAME} found in {} or any parent directory",
            root.display()
        ),
        Missing::Create => Ok(start.join(DATABASE_NAME)),
    }
}

/// `path` made absolute with `.` and `..` resolved lexically.
fn absolute(path: &Path) -> Result<PathBuf> {
    let absolute = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
    let mut normal = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            component => normal.push(component),
        }
    }
    Ok(normal)
}
