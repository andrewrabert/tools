//! Running ffmpeg and ffprobe.
use std::fmt;
use std::num::NonZeroU64;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Error, Result, anyhow, bail};
use serde::Deserialize;

/// An ffmpeg command that never reads stdin and prints no banner.
pub fn command() -> Command {
    let mut command = Command::new("ffmpeg");
    command.args(["-nostdin", "-hide_banner"]);
    command
}

/// Runs `command` to completion, failing unless it exits successfully.
pub fn run(command: &mut Command) -> Result<()> {
    let status = command.status().context("running ffmpeg")?;
    if !status.success() {
        bail!("ffmpeg failed: {status}");
    }
    Ok(())
}

/// Runs `command` to completion with its stdout and stderr captured, failing unless it exits
/// successfully.
pub fn output(command: &mut Command) -> Result<Output> {
    let output = command.output().context("running ffmpeg")?;
    if !output.status.success() {
        bail!(
            "ffmpeg failed: {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim_end()
        );
    }
    Ok(output)
}

/// A nonzero frames-per-second rational, as ffmpeg writes it (`24000/1001`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRate {
    frames: NonZeroU64,
    seconds: NonZeroU64,
}

impl FromStr for FrameRate {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        let invalid = || anyhow!("invalid frame rate {value:?}");
        let (frames, seconds) = value.split_once('/').ok_or_else(invalid)?;
        Ok(FrameRate {
            frames: frames.parse().map_err(|_| invalid())?,
            seconds: seconds.parse().map_err(|_| invalid())?,
        })
    }
}

impl fmt::Display for FrameRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.frames, self.seconds)
    }
}

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Stream {
    #[serde(default)]
    codec_type: String,
    avg_frame_rate: Option<String>,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

/// The average frame rate of the first video stream in the media file at `path` that has one.
pub fn video_frame_rate(path: &Path) -> Result<FrameRate> {
    probe(path, &["-show_streams"])?
        .streams
        .into_iter()
        .filter(|stream| stream.codec_type == "video")
        .find_map(|stream| stream.avg_frame_rate?.parse().ok())
        .with_context(|| format!("{} has no video stream with a frame rate", path.display()))
}

/// The duration of the media file at `path`.
pub fn duration(path: &Path) -> Result<Duration> {
    let duration = probe(path, &["-show_entries", "format=duration"])?
        .format
        .and_then(|format| format.duration)
        .with_context(|| format!("{} has no duration", path.display()))?;
    duration
        .parse()
        .ok()
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .with_context(|| format!("invalid duration {duration:?}"))
}

fn probe(path: &Path, show: &[&str]) -> Result<Probe> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-print_format", "json"])
        .args(show)
        .arg("--")
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .context("running ffprobe")?;
    if !output.status.success() {
        bail!("ffprobe failed on {}: {}", path.display(), output.status);
    }
    serde_json::from_slice(&output.stdout).context("parsing the output of ffprobe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_rate_round_trips() -> Result<()> {
        assert_eq!("24000/1001".parse::<FrameRate>()?.to_string(), "24000/1001");
        Ok(())
    }

    #[test]
    fn frame_rate_rejects_zero() {
        assert!("0/0".parse::<FrameRate>().is_err());
        assert!("25/0".parse::<FrameRate>().is_err());
    }

    #[test]
    fn frame_rate_rejects_missing_slash() {
        assert!("25".parse::<FrameRate>().is_err());
    }
}
