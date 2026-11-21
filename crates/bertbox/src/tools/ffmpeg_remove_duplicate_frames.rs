use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::ffmpeg::{self, FrameRate};
use crate::pool;
use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "Remove duplicate frames from videos in place")]
pub struct FfmpegRemoveDuplicateFrames {
    #[arg(
        short = 'n',
        long = "num-concurrent",
        value_name = "NUM",
        default_value_t = 1,
        help = "number of parallel jobs, 0 for cpu count"
    )]
    num_procs: usize,
    /// Keep up to NUM identical frames in a row, 0 to remove every near-duplicate frame
    #[arg(short = 'd', long, value_name = "NUM", default_value_t = 0)]
    max_dupes: usize,
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
}

impl Tool for FfmpegRemoveDuplicateFrames {
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

/// Which duplicate frames are removed.
#[derive(Clone, Copy)]
enum Removal {
    /// Every frame ffmpeg's mpdecimate finds near-identical to the one before.
    NearDuplicates,
    /// Identical frames past the first `n` in a row.
    IdenticalPast(NonZeroUsize),
}

impl From<usize> for Removal {
    fn from(max_dupes: usize) -> Self {
        match NonZeroUsize::new(max_dupes) {
            Some(max_dupes) => Removal::IdenticalPast(max_dupes),
            None => Removal::NearDuplicates,
        }
    }
}

/// True when every video was processed.
fn run(args: FfmpegRemoveDuplicateFrames) -> Result<bool> {
    let removal = Removal::from(args.max_dupes);
    let workers = pool::jobs(args.num_procs)?;
    let mut ok = true;
    pool::run(
        args.paths,
        workers,
        |path| remove_duplicates(path, removal),
        |path, result| {
            if let Err(error) = result {
                eprintln!("Error: {}: {error:?}", path.display());
                ok = false;
            }
            Ok(())
        },
    )?;
    Ok(ok)
}

fn remove_duplicates(path: &Path, removal: Removal) -> Result<()> {
    let frame_rate = ffmpeg::video_frame_rate(path)?;
    let select = match removal {
        Removal::NearDuplicates => "mpdecimate".to_owned(),
        Removal::IdenticalPast(max_dupes) => {
            let ranges = drop_ranges(&frame_hashes(path)?, max_dupes);
            if ranges.is_empty() {
                return Ok(());
            }
            select_filter(&ranges)
        }
    };
    let temp = ffmpeg::temp_beside(path)?;
    ffmpeg::output(
        ffmpeg::command()
            .arg("-i")
            .arg(path)
            .arg("-vf")
            .arg(format!("{select},{}", setpts_filter(frame_rate)))
            .arg("-y")
            .arg(&temp)
            .stdout(Stdio::null()),
    )?;
    temp.persist(path)
        .with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// The MD5 of each frame of the first video stream in `path`, in order.
fn frame_hashes(path: &Path) -> Result<Vec<String>> {
    let output = ffmpeg::output(
        ffmpeg::command()
            .arg("-i")
            .arg(path)
            .args(["-map", "0:v:0", "-f", "framemd5", "-"]),
    )?;
    let stdout = String::from_utf8(output.stdout).context("reading the frame hashes")?;
    Ok(stdout
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| line.rsplit(',').next())
        .map(|hash| hash.trim().to_owned())
        .collect())
}

/// An inclusive span of frame indices to drop.
#[derive(Debug, PartialEq, Eq)]
struct DropRange {
    first: usize,
    last: usize,
}

/// The frames past the first `max_dupes` of each run of identical frames.
fn drop_ranges(hashes: &[String], max_dupes: NonZeroUsize) -> Vec<DropRange> {
    let max_dupes = max_dupes.get();
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < hashes.len() {
        let run = hashes[start..]
            .iter()
            .take_while(|hash| **hash == hashes[start])
            .count();
        if run > max_dupes {
            ranges.push(DropRange {
                first: start + max_dupes,
                last: start + run - 1,
            });
        }
        start += run;
    }
    ranges
}

fn select_filter(ranges: &[DropRange]) -> String {
    let between = ranges
        .iter()
        .map(|range| format!("between(n,{},{})", range.first, range.last))
        .collect::<Vec<_>>()
        .join("+");
    format!("select='not({between})'")
}

/// Retimes the remaining frames to play back to back at `frame_rate`.
fn setpts_filter(frame_rate: FrameRate) -> String {
    format!("setpts=N/({frame_rate})/TB")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashes(values: &str) -> Vec<String> {
        values.chars().map(String::from).collect()
    }

    fn nonzero(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap()
    }

    #[test]
    fn no_repeats_drop_nothing() {
        assert_eq!(drop_ranges(&hashes("abc"), nonzero(1)), []);
    }

    #[test]
    fn repeats_within_the_limit_are_kept() {
        assert_eq!(drop_ranges(&hashes("aab"), nonzero(2)), []);
    }

    #[test]
    fn repeats_past_the_limit_are_dropped() {
        assert_eq!(
            drop_ranges(&hashes("aaaabccc"), nonzero(2)),
            [
                DropRange { first: 2, last: 3 },
                DropRange { first: 7, last: 7 }
            ]
        );
    }

    #[test]
    fn select_filter_excludes_every_range() {
        let ranges = [
            DropRange { first: 2, last: 3 },
            DropRange { first: 7, last: 7 },
        ];
        assert_eq!(
            select_filter(&ranges),
            "select='not(between(n,2,3)+between(n,7,7))'"
        );
    }

    #[test]
    fn setpts_filter_uses_the_exact_rate() -> Result<()> {
        assert_eq!(
            setpts_filter("24000/1001".parse()?),
            "setpts=N/(24000/1001)/TB"
        );
        Ok(())
    }
}
