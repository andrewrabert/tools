use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Args as ClapArgs};

use crate::mkvmerge::{self, Track, TrackType};
use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "List Matroska files by the languages of their audio and subtitle tracks")]
#[command(group(ArgGroup::new("mode").required(true).args(["exclude", "include"])))]
pub struct MkvFindLang {
    /// List files with a language other than LANGUAGE. May be repeated
    #[arg(short, long, value_name = "LANGUAGE", value_parser = parse_language)]
    exclude: Vec<String>,
    /// List files with LANGUAGE. May be repeated
    #[arg(short, long, value_name = "LANGUAGE", value_parser = parse_language)]
    include: Vec<String>,
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
}

impl Tool for MkvFindLang {
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

/// An ISO 639-2 language code, which Matroska uses.
fn parse_language(value: &str) -> Result<String, String> {
    if value.chars().count() == 3 {
        Ok(value.to_owned())
    } else {
        Err("must be a three-letter ISO 639-2 code".to_owned())
    }
}

fn run(args: MkvFindLang) -> Result<()> {
    let exclude = !args.exclude.is_empty();
    let languages: BTreeSet<String> = if exclude { args.exclude } else { args.include }
        .into_iter()
        .collect();
    let files = mkvmerge::find_files(&args.paths)?;
    if files.is_empty() {
        bail!("no mkv files found");
    }
    for path in files {
        let identify =
            mkvmerge::identify(&path).with_context(|| format!("identifying {}", path.display()))?;
        let found = identify
            .tracks
            .iter()
            .filter(|track| matches!(track.kind, TrackType::Audio | TrackType::Subtitles))
            .map(Track::language)
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("reading the languages of {}", path.display()))?;
        let matched = if exclude {
            found.iter().any(|language| !languages.contains(*language))
        } else {
            found.iter().any(|language| languages.contains(*language))
        };
        if matched {
            println!("{}", path.display());
        }
    }
    Ok(())
}
