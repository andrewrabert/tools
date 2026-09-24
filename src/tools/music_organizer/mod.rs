mod album;
mod cli;
mod files;
mod log;
mod name;
mod tags;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{self, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::tools::Tool;
use crate::tools::music_organizer::album::Options;
pub use crate::tools::music_organizer::cli::MusicOrganizer;
use crate::tools::music_organizer::log::Log;

const LIBRARY: &str = "/storage/Audio/Library";

impl Tool for MusicOrganizer {
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

fn run(args: MusicOrganizer) -> Result<()> {
    let directory = path::absolute(&args.directory)
        .with_context(|| format!("resolving {}", args.directory.display()))?;
    let organize_dirs = match &args.organize_dirs {
        Some(dir) => {
            Some(path::absolute(dir).with_context(|| format!("resolving {}", dir.display()))?)
        }
        None => None,
    };
    let options = Options {
        scan_root: &directory,
        organize_dirs: organize_dirs.as_deref(),
        library: args.library.then_some(Path::new(LIBRARY)),
        dry_run: args.dry_run,
    };
    let mut log = Log::open()?;

    let mut artists = BTreeSet::new();
    for dir in all_dirs(&directory)? {
        files::set_permissions(&dir)?;
        files::rename_extensions(&mut log, &dir, args.dry_run)?;
        files::organize_images(&mut log, &dir, args.dry_run)?;
        files::check_cue_files(&mut log, &dir, args.dry_run)?;
        if args.check_tags || organize_dirs.is_some() {
            artists.extend(album::organize(&mut log, &dir, &options)?);
        }
    }

    audit_artist_variants(&mut log, &artists);
    Ok(())
}

fn all_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = vec![root.to_path_buf()];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry
                .with_context(|| format!("reading {}", dir.display()))?
                .path();
            if path.is_dir() {
                dirs.push(path.clone());
                stack.push(path);
            }
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Artists whose names differ only in case, punctuation or spacing.
fn audit_artist_variants(log: &mut Log, artists: &BTreeSet<String>) {
    let mut variants: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for artist in artists {
        let normalized: String = artist
            .to_lowercase()
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        if !normalized.is_empty() {
            variants.entry(normalized).or_default().insert(artist);
        }
    }
    for (normalized, names) in variants {
        if names.len() > 1 {
            log.error(format!("artist variants: {normalized} - {names:?}"));
        }
    }
}
