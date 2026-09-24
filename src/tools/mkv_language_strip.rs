use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use indicatif::ProgressBar;
use tempfile::NamedTempFile;

use crate::mkvmerge::{self, Language, Track, TrackType};
use crate::tools::Tool;

const DEFAULT_LANGUAGES: [Language; 3] = [
    Language::English,
    Language::Undetermined,
    Language::NonLinguistic,
];

#[derive(ClapArgs)]
#[command(about = "Remove audio and subtitle tracks in other languages from Matroska files")]
pub struct MkvLanguageStrip {
    /// Language to keep. May be repeated (default: eng, und, zxx)
    #[arg(short, long = "keep", value_name = "LANGUAGE")]
    keep: Vec<String>,
    /// List the tracks that would be removed without removing them
    #[arg(short, long)]
    dryrun: bool,
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
}

impl Tool for MkvLanguageStrip {
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

struct Strip {
    path: PathBuf,
    audio: Vec<u64>,
    subtitles: Vec<u64>,
}

fn run(args: MkvLanguageStrip) -> Result<()> {
    let keep: Vec<&str> = if args.keep.is_empty() {
        DEFAULT_LANGUAGES.iter().map(Language::as_str).collect()
    } else {
        args.keep.iter().map(String::as_str).collect()
    };
    let files = mkvmerge::find_files(&args.paths)?;
    if files.is_empty() {
        bail!("no mkv files found");
    }

    let mut pending = Vec::new();
    for path in files {
        let identify =
            mkvmerge::identify(&path).with_context(|| format!("identifying {}", path.display()))?;
        let languages = identify
            .tracks
            .iter()
            .map(Track::language)
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("reading the languages of {}", path.display()))?;
        let removed = |kind: TrackType| -> Vec<u64> {
            identify
                .tracks
                .iter()
                .zip(&languages)
                .filter(|(track, language)| track.kind == kind && !keep.contains(*language))
                .map(|(track, _)| track.id)
                .collect()
        };
        let strip = Strip {
            audio: removed(TrackType::Audio),
            subtitles: removed(TrackType::Subtitles),
            path,
        };
        if !strip.audio.is_empty() || !strip.subtitles.is_empty() {
            pending.push(strip);
        }
    }

    let progress = ProgressBar::new(pending.len() as u64);
    for strip in &pending {
        progress.println(describe(strip));
        if !args.dryrun {
            remove_tracks(strip)?;
        }
        progress.inc(1);
    }
    progress.finish();
    Ok(())
}

fn describe(strip: &Strip) -> String {
    let mut line = strip.path.display().to_string();
    if !strip.audio.is_empty() {
        line.push_str(&format!(" (audio: {:?})", strip.audio));
    }
    if !strip.subtitles.is_empty() {
        line.push_str(&format!(" (subtitle: {:?})", strip.subtitles));
    }
    line
}

fn remove_tracks(strip: &Strip) -> Result<()> {
    let parent = strip
        .path
        .parent()
        .context("file has no parent directory")?;
    let temp = NamedTempFile::new_in(parent)
        .with_context(|| format!("creating a temporary file in {}", parent.display()))?;
    let mut command = Command::new("mkvmerge");
    command.arg("-o").arg(temp.path());
    exclude_tracks(&mut command, "--audio-tracks", &strip.audio);
    exclude_tracks(&mut command, "--subtitle-tracks", &strip.subtitles);
    let status = command
        .arg(&strip.path)
        .stdout(Stdio::null())
        .status()
        .context("running mkvmerge")?;
    if !status.success() {
        bail!("mkvmerge failed on {}: {status}", strip.path.display());
    }
    temp.persist(&strip.path)
        .with_context(|| format!("replacing {}", strip.path.display()))?;
    Ok(())
}

fn exclude_tracks(command: &mut Command, option: &str, tracks: &[u64]) {
    if tracks.is_empty() {
        return;
    }
    let ids: Vec<String> = tracks.iter().map(u64::to_string).collect();
    command.arg(option).arg(format!("!{}", ids.join(",")));
}
