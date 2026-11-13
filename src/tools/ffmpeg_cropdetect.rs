use std::ffi::OsString;
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::process::{ExitCode, Stdio};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use serde::Serialize;

use crate::ffmpeg;
use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "Detect the crop that removes a video's black borders, as JSON")]
pub struct FfmpegCropdetect {
    /// Number of evenly spaced samples to take across the video
    #[arg(long, value_name = "NUM", default_value = "20")]
    steps: NonZeroU64,
    /// Length of each sample, as an ffmpeg time duration
    #[arg(long, value_name = "DURATION", default_value = "00:05")]
    duration: OsString,
    #[arg(value_name = "PATH")]
    path: PathBuf,
}

impl Tool for FfmpegCropdetect {
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

/// A crop rectangle, as ffmpeg's cropdetect writes it (`crop=W:H:X:Y`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct Crop {
    width: u64,
    height: u64,
    x: u64,
    y: u64,
}

impl Crop {
    fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split(':').map(str::parse);
        let crop = Crop {
            width: parts.next()?.ok()?,
            height: parts.next()?.ok()?,
            x: parts.next()?.ok()?,
            y: parts.next()?.ok()?,
        };
        parts.next().is_none().then_some(crop)
    }

    /// The smallest crop that contains both.
    fn union(self, other: Crop) -> Crop {
        Crop {
            width: self.width.max(other.width),
            height: self.height.max(other.height),
            x: self.x.min(other.x),
            y: self.y.min(other.y),
        }
    }
}

fn run(args: FfmpegCropdetect) -> Result<()> {
    let seconds = ffmpeg::duration(&args.path)?.as_secs();
    let step = (seconds / args.steps).max(1);
    let mut crop: Option<Crop> = None;
    for start in (0..seconds).step_by(usize::try_from(step).context("step too large")?) {
        let output = ffmpeg::output(
            ffmpeg::command()
                .arg("-ss")
                .arg(start.to_string())
                .arg("-t")
                .arg(&args.duration)
                .arg("-i")
                .arg(&args.path)
                .args([
                    "-max_muxing_queue_size",
                    "1024",
                    "-fps_mode",
                    "vfr",
                    "-vf",
                    "cropdetect",
                    "-f",
                    "null",
                    "-",
                ])
                .stdout(Stdio::null()),
        )?;
        for found in crops(&String::from_utf8_lossy(&output.stderr)) {
            crop = Some(crop.map_or(found, |crop| crop.union(found)));
        }
    }
    let crop = crop.with_context(|| format!("no crop detected in {}", args.path.display()))?;
    println!("{}", serde_json::to_string(&crop)?);
    Ok(())
}

/// The crops cropdetect logged in ffmpeg's stderr.
fn crops(stderr: &str) -> impl Iterator<Item = Crop> {
    stderr
        .lines()
        .filter(|line| line.starts_with("[Parsed_cropdetect_"))
        .flat_map(str::split_whitespace)
        .filter_map(|part| part.strip_prefix("crop="))
        .filter_map(Crop::parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_are_read_from_cropdetect_lines() {
        let stderr = "\
frame=1
[Parsed_cropdetect_0 @ 0x1] x1:0 x2:1919 y1:140 y2:939 w:1920 h:800 x:0 y:140 pts:0 t:0.0 limit:0.094 crop=1920:800:0:140
[other @ 0x2] crop=1:1:1:1
";
        assert_eq!(
            crops(stderr).collect::<Vec<_>>(),
            [Crop {
                width: 1920,
                height: 800,
                x: 0,
                y: 140
            }]
        );
    }

    #[test]
    fn crop_rejects_other_shapes() {
        assert_eq!(Crop::parse("1:2:3"), None);
        assert_eq!(Crop::parse("1:2:3:4:5"), None);
        assert_eq!(Crop::parse("1:2:3:-4"), None);
    }

    #[test]
    fn union_contains_both() {
        let a = Crop {
            width: 1920,
            height: 800,
            x: 0,
            y: 140,
        };
        let b = Crop {
            width: 1900,
            height: 816,
            x: 10,
            y: 132,
        };
        assert_eq!(
            a.union(b),
            Crop {
                width: 1920,
                height: 816,
                x: 0,
                y: 132
            }
        );
    }
}
