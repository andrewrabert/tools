use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

use anyhow::{Context, Error, Result, anyhow, bail};
use clap::Args as ClapArgs;
use tempfile::Builder;

use crate::ffmpeg;
use crate::tools::Tool;

const TEMP_PREFIX: &str = ".ffmpeg_extract_slice";

#[derive(ClapArgs)]
#[command(about = "Extract a span of a video, without audio, as lossless FFV1")]
pub struct FfmpegExtractSlice {
    /// The file to write, which must not exist
    #[arg(short = 'o', long, value_name = "OUTPUT")]
    output: PathBuf,
    /// Where the slice starts (HH:MM:SS)
    #[arg(long, value_name = "TIME")]
    start: Timestamp,
    /// Where the slice ends (HH:MM:SS)
    #[arg(long, value_name = "TIME")]
    end: Timestamp,
    #[arg(value_name = "SOURCE")]
    source: PathBuf,
}

impl Tool for FfmpegExtractSlice {
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

/// A position in a video, to the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Timestamp {
    hours: u64,
    minutes: u8,
    seconds: u8,
}

impl FromStr for Timestamp {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        let invalid = || anyhow!("expected HH:MM:SS, got {value:?}");
        let mut parts = value.split(':');
        let (Some(hours), Some(minutes), Some(seconds), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(invalid());
        };
        let hours = hours.parse().map_err(|_| invalid())?;
        let minutes = minutes.parse().map_err(|_| invalid())?;
        let seconds = seconds.parse().map_err(|_| invalid())?;
        if minutes > 59 {
            bail!("minutes must be 0 to 59, got {minutes}");
        }
        if seconds > 59 {
            bail!("seconds must be 0 to 59, got {seconds}");
        }
        Ok(Timestamp {
            hours,
            minutes,
            seconds,
        })
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02}:{:02}:{:02}",
            self.hours, self.minutes, self.seconds
        )
    }
}

fn run(args: FfmpegExtractSlice) -> Result<()> {
    if !args.source.is_file() {
        bail!("{} is not a file", args.source.display());
    }
    if args.output.exists() {
        bail!("{} already exists", args.output.display());
    }
    if args.end <= args.start {
        bail!("the end {} is not after the start {}", args.end, args.start);
    }

    let dir = args
        .output
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // ffmpeg picks the container from the extension, so the temporary file keeps it.
    let suffix = args
        .output
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let temp = Builder::new()
        .prefix(TEMP_PREFIX)
        .suffix(&suffix)
        .tempfile_in(dir)
        .with_context(|| format!("creating a temporary file in {}", dir.display()))?
        .into_temp_path();
    ffmpeg::run(
        ffmpeg::command()
            .arg("-ss")
            .arg(args.start.to_string())
            .arg("-to")
            .arg(args.end.to_string())
            .arg("-i")
            .arg(&args.source)
            .args(["-an", "-vcodec", "ffv1", "-slices", "30", "-y"])
            .arg(&temp),
    )?;
    temp.persist_noclobber(&args.output)
        .with_context(|| format!("writing {}", args.output.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_round_trips() -> Result<()> {
        assert_eq!("1:02:03".parse::<Timestamp>()?.to_string(), "01:02:03");
        Ok(())
    }

    #[test]
    fn timestamp_allows_hours_past_a_day() -> Result<()> {
        assert_eq!("100:00:00".parse::<Timestamp>()?.to_string(), "100:00:00");
        Ok(())
    }

    #[test]
    fn timestamp_rejects_minutes_past_59() {
        assert!("00:60:00".parse::<Timestamp>().is_err());
    }

    #[test]
    fn timestamp_rejects_seconds_past_59() {
        assert!("00:00:60".parse::<Timestamp>().is_err());
    }

    #[test]
    fn timestamp_rejects_other_shapes() {
        for value in ["00:00", "00:00:00:00", "-1:00:00", "a:00:00", ""] {
            assert!(value.parse::<Timestamp>().is_err(), "{value}");
        }
    }

    #[test]
    fn timestamps_order_by_position() -> Result<()> {
        assert!("00:59:59".parse::<Timestamp>()? < "01:00:00".parse::<Timestamp>()?);
        Ok(())
    }
}
