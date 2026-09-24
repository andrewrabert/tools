use std::collections::BTreeSet;
use std::fs;
use std::path::{self, Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use tempfile::Builder;

use crate::mkvmerge::EXTENSION;
use crate::tools::Tool;
use crate::{pool, walk};

const VIDEO_EXTENSIONS: [&str; 11] = [
    "avi", "flv", "m2ts", "m4v", "mov", "mp4", "mpeg", "mpg", "ts", "vob", "webm",
];
const SUBTITLE_EXTENSIONS: [&str; 4] = ["ass", "idx", "srt", "sub"];

#[derive(ClapArgs)]
#[command(about = "Merge videos and their sidecar subtitles into Matroska files")]
pub struct MkvConvert {
    /// Remux .mkv files even when they have no sidecar subtitles
    #[arg(long)]
    force: bool,
    /// Keep the original files after conversion
    #[arg(long)]
    keep: bool,
    #[arg(
        short = 'n',
        long = "num-concurrent",
        value_name = "NUM",
        default_value_t = 1,
        help = "number of parallel jobs, 0 for cpu count"
    )]
    num_procs: usize,
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
}

impl Tool for MkvConvert {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

struct Job {
    dest: PathBuf,
    /// The video first, then its subtitles.
    sources: Vec<PathBuf>,
    title: String,
}

/// True when every conversion succeeded.
fn run(args: MkvConvert) -> Result<bool> {
    let mut ok = true;
    let mut jobs = Vec::new();
    for path in files(&args.paths)? {
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase());
        let is_video = extension.as_deref().is_some_and(|extension| {
            extension == EXTENSION || VIDEO_EXTENSIONS.contains(&extension)
        });
        if !is_video {
            eprintln!(
                "unrecognized extension: \"{}\" ({})",
                extension.unwrap_or_default(),
                path.display()
            );
            ok = false;
            continue;
        }
        let dest = path.with_extension(EXTENSION);
        let subtitles = subtitles(&path)?;
        if args.force || dest != path || !subtitles.is_empty() {
            let title = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut sources = vec![path];
            sources.extend(subtitles);
            jobs.push(Job {
                dest,
                sources,
                title,
            });
        }
    }

    let workers = pool::jobs(args.num_procs)?;
    pool::run(
        jobs,
        workers,
        |job| merge(job, !args.keep),
        |job, result| {
            if let Err(error) = result {
                eprintln!("error: {}: {error:?}", job.dest.display());
                ok = false;
            }
            Ok(())
        },
    )?;
    Ok(ok)
}

/// The regular files among and under `paths`, absolute and sorted. Symlinked files are skipped.
///
/// Absolute, since mkvmerge accepts no `--` to end its options.
fn files(paths: &[PathBuf]) -> Result<BTreeSet<PathBuf>> {
    let mut files = BTreeSet::new();
    for path in paths {
        let path = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        for entry in walk::entries(&path) {
            let entry = entry?;
            if entry.file_type().is_file() && !entry.path_is_symlink() {
                files.insert(entry.into_path());
            }
        }
    }
    Ok(files)
}

/// The subtitle files beside `video` whose names start with its stem, ignoring case.
fn subtitles(video: &Path) -> Result<Vec<PathBuf>> {
    let (Some(parent), Some(stem)) = (video.parent(), video.file_stem()) else {
        return Ok(Vec::new());
    };
    let stem = stem.to_string_lossy().to_lowercase();
    let mut subtitles = Vec::new();
    let entries = fs::read_dir(parent).with_context(|| format!("reading {}", parent.display()))?;
    for entry in entries {
        let path = entry
            .with_context(|| format!("reading {}", parent.display()))?
            .path();
        let name_matches = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().to_lowercase().starts_with(&stem));
        let is_subtitle = path.extension().is_some_and(|extension| {
            SUBTITLE_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        });
        if name_matches && is_subtitle && path.is_file() {
            subtitles.push(path);
        }
    }
    subtitles.sort();
    Ok(subtitles)
}

fn merge(job: &Job, delete_sources: bool) -> Result<()> {
    println!("converting {}", job.dest.display());
    let parent = job
        .dest
        .parent()
        .context("destination has no parent directory")?;
    let temp = Builder::new()
        .prefix(".convert-to-mkv_")
        .suffix(".mkv.tmp")
        .tempfile_in(parent)
        .with_context(|| format!("creating a temporary file in {}", parent.display()))?;
    let status = Command::new("mkvmerge")
        .arg("--quiet")
        .arg("--output")
        .arg(temp.path())
        .arg("--title")
        .arg(&job.title)
        .args(&job.sources)
        .status()
        .context("running mkvmerge")?;
    if !status.success() {
        bail!("mkvmerge failed: {status}");
    }
    temp.persist(&job.dest)
        .with_context(|| format!("replacing {}", job.dest.display()))?;
    if delete_sources {
        for source in job.sources.iter().filter(|source| **source != job.dest) {
            fs::remove_file(source).with_context(|| format!("removing {}", source.display()))?;
        }
    }
    Ok(())
}
