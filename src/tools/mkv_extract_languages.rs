use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{self, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::mkvmerge::{self, Language, TrackType};
use crate::tools::Tool;

const LANGUAGES: [Language; 2] = [Language::English, Language::Undetermined];

#[derive(ClapArgs)]
#[command(about = "Extract the English and undetermined tracks of a Matroska file")]
pub struct MkvExtractLanguages {
    /// Extract subtitle tracks
    #[arg(long)]
    subtitle: bool,
    /// Extract audio tracks
    #[arg(long)]
    audio: bool,
    /// Extract video tracks
    #[arg(long)]
    video: bool,
    #[arg(value_name = "PATH")]
    path: PathBuf,
}

impl Tool for MkvExtractLanguages {
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

/// Replaces this process with mkvextract, so returns only on failure.
fn run(args: MkvExtractLanguages) -> Result<()> {
    // Absolute, since mkvextract accepts no `--` to end its options.
    let path =
        path::absolute(&args.path).with_context(|| format!("resolving {}", args.path.display()))?;
    let kinds = [
        (args.subtitle, TrackType::Subtitles),
        (args.audio, TrackType::Audio),
        (args.video, TrackType::Video),
    ];
    let identify = mkvmerge::identify(&path)?;
    let mut command = Command::new("mkvextract");
    command.arg(&path).arg("tracks");
    for (_, kind) in kinds.iter().filter(|(selected, _)| *selected) {
        for track in &identify.tracks {
            if track.kind != *kind {
                continue;
            }
            let language = track.language()?;
            if LANGUAGES.iter().any(|known| known.as_str() == language) {
                let mut spec = OsString::from(format!("{}:", track.id));
                spec.push(&path);
                spec.push(format!(".track_{}", track.id));
                command.arg(spec);
            }
        }
    }
    Err(command.exec()).context("running mkvextract")
}
