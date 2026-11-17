//! The track-extract subcommand.
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, ensure};

use crate::cue::extract::{self, Options};
use crate::tools::cue_tool::cli::TrackExtract;

const BIN_EXTENSION: &str = "bin";

pub fn run(args: &TrackExtract) -> ExitCode {
    match tracks(args) {
        Ok(written) => {
            for path in written {
                println!("Wrote {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn tracks(args: &TrackExtract) -> Result<Vec<PathBuf>> {
    let bin = match &args.bin {
        Some(bin) => bin.clone(),
        None => args.cuefile.with_extension(BIN_EXTENSION),
    };
    let basename = basename(args)?;
    if let Some(outdir) = &args.outdir {
        fs::create_dir_all(outdir).with_context(|| format!("creating {}", outdir.display()))?;
    }
    extract::tracks(
        &bin,
        &args.cuefile,
        &basename,
        &Options {
            raw: args.raw,
            psx: args.psx,
            wav: args.wav,
            swap_audio: args.swap_audio,
            force: args.force,
        },
    )
}

fn basename(args: &TrackExtract) -> Result<OsString> {
    let name = match &args.name {
        Some(name) => {
            ensure!(
                !name.is_empty()
                    && Path::new(name)
                        .file_name()
                        .is_some_and(|file| file == name.as_str()),
                "{name:?} is not a file name"
            );
            OsStr::new(name)
        }
        None => args
            .cuefile
            .file_stem()
            .with_context(|| format!("{} has no file name", args.cuefile.display()))?,
    };
    let dir = match &args.outdir {
        Some(outdir) => outdir.as_path(),
        None => args.cuefile.parent().unwrap_or(Path::new("")),
    };
    Ok(dir.join(name).into_os_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUE: &str = "FILE \"image.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 01 00:00:01\n";

    fn args(cuefile: PathBuf) -> TrackExtract {
        TrackExtract {
            cuefile,
            bin: None,
            outdir: None,
            name: None,
            raw: false,
            psx: false,
            wav: false,
            swap_audio: false,
            force: false,
        }
    }

    /// A directory holding `image.cue` and the two sectors of `image.bin`.
    fn image() -> Result<tempfile::TempDir> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join("image.bin"), vec![0; 2 * 2352])?;
        fs::write(dir.path().join("image.cue"), CUE)?;
        Ok(dir)
    }

    #[test]
    fn tracks_go_beside_the_cue_sheet_under_its_name() -> Result<()> {
        let dir = image()?;

        let written = tracks(&args(dir.path().join("image.cue")))?;

        assert_eq!(
            written,
            [
                dir.path().join("image01.iso"),
                dir.path().join("image02.cdr")
            ]
        );
        assert!(written.iter().all(|path| path.exists()));
        Ok(())
    }

    #[test]
    fn bin_outdir_and_name_replace_the_defaults() -> Result<()> {
        let dir = image()?;
        let moved = dir.path().join("moved.bin");
        fs::rename(dir.path().join("image.bin"), &moved)?;
        let outdir = dir.path().join("new").join("tracks");

        let written = tracks(&TrackExtract {
            bin: Some(moved),
            outdir: Some(outdir.clone()),
            name: Some("disc".to_owned()),
            ..args(dir.path().join("image.cue"))
        })?;

        assert_eq!(
            written,
            [outdir.join("disc01.iso"), outdir.join("disc02.cdr")]
        );
        assert!(written.iter().all(|path| path.exists()));
        Ok(())
    }

    #[test]
    fn name_must_be_a_file_name() {
        let named = |name: &str| TrackExtract {
            name: Some(name.to_owned()),
            ..args(PathBuf::from("image.cue"))
        };
        assert!(basename(&named("")).is_err());
        assert!(basename(&named("a/b")).is_err());
        assert!(basename(&named("..")).is_err());
        assert_eq!(basename(&named("disc")).ok(), Some(OsString::from("disc")));
    }
}
