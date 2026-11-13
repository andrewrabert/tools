use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, Result, bail, ensure};
use clap::{Args as ClapArgs, ValueEnum};
use tempfile::{Builder, TempDir};

use crate::ffmpeg;
use crate::tools::Tool;
use crate::tools::rife;

const TEMP_PREFIX: &str = ".img-to-interpolated-video";

#[derive(ClapArgs)]
#[command(about = "Render a video or directory of frames as a looping, interpolated animation")]
pub struct ImgToInterpolatedVideo {
    /// The file to write, or the directory for png; defaults to a timestamp
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,
    /// How many extra times mp4 and webm output repeats the frames
    #[arg(short, long, value_name = "NUM", default_value_t = 3)]
    loops: u32,
    /// How many times rife doubles the frame count, 0 for no interpolation
    #[arg(short, long, value_name = "NUM", default_value_t = 0)]
    rife_iterations: u32,
    /// The rife model
    #[arg(long, value_name = "MODEL", default_value = rife::DEFAULT_MODEL)]
    rife_model: String,
    /// Average each NUM consecutive frames into one (motion blur)
    #[arg(long, value_name = "NUM")]
    blend: Option<NonZeroUsize>,
    /// The frame rate of the output
    #[arg(long, value_name = "FPS", default_value = "30")]
    fps: String,
    /// Resize the frames by FACTOR (0.5 is half size)
    #[arg(long, value_name = "FACTOR")]
    scale: Option<f64>,
    #[arg(short, long, value_enum, default_value_t = Mode::Oneshot)]
    mode: Mode,
    /// Where to start reading a video source (ffmpeg -ss)
    #[arg(long, value_name = "TIME")]
    ss: Option<String>,
    /// Where to stop reading a video source (ffmpeg -to)
    #[arg(long, value_name = "TIME")]
    to: Option<String>,
    #[arg(short, long, value_enum, default_value_t = Format::Mp4)]
    format: Format,
    /// A video file, or a directory of frames in name order
    #[arg(value_name = "SOURCE")]
    source: PathBuf,
}

impl Tool for ImgToInterpolatedVideo {
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

/// How the frames play back.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Mode {
    /// Forward, then backward to the first frame
    Bounce,
    /// Forward, interpolating from the last frame back to the first
    Cycle,
    /// Forward once
    Oneshot,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Gif,
    Mp4,
    Webm,
    Webp,
    /// A directory of the final frames
    Png,
}

impl Format {
    fn extension(self) -> Option<&'static str> {
        match self {
            Format::Gif => Some("gif"),
            Format::Mp4 => Some("mp4"),
            Format::Webm => Some("webm"),
            Format::Webp => Some("webp"),
            Format::Png => None,
        }
    }

    /// The ffmpeg output options; `-an` drops audio so players (Signal, for one) treat the
    /// output as a looping animation.
    fn codec_args(self) -> Option<&'static [&'static str]> {
        match self {
            Format::Mp4 => Some(&[
                "-an",
                "-c:v",
                "libx264",
                "-preset",
                "slow",
                "-crf",
                "16",
                "-movflags",
                "+faststart",
            ]),
            Format::Webm => Some(&["-an", "-c:v", "libvpx-vp9", "-crf", "30", "-b:v", "0"]),
            Format::Webp => Some(&[
                "-an",
                "-c:v",
                "libwebp_anim",
                "-loop",
                "0",
                "-quality",
                "90",
            ]),
            Format::Gif | Format::Png => None,
        }
    }
}

fn run(args: ImgToInterpolatedVideo) -> Result<()> {
    if let Some(scale) = args.scale {
        ensure!(
            scale.is_finite() && scale > 0.0,
            "the scale must be positive, got {scale}"
        );
    }
    let output = args.output.unwrap_or_else(|| default_output(args.format));

    // Frames extracted from a video live in the system temporary directory; every later stage
    // lives beside the frames it reads so that nothing crosses a filesystem.
    let extracted;
    let source_frames = if args.source.is_file() {
        extracted = Builder::new()
            .prefix(TEMP_PREFIX)
            .tempdir()
            .context("creating a temporary directory")?;
        eprintln!("Extracting frames");
        extract_frames(
            &args.source,
            extracted.path(),
            args.ss.as_deref(),
            args.to.as_deref(),
        )?;
        extracted.path()
    } else if args.source.is_dir() {
        args.source.as_path()
    } else {
        bail!("{} is not a file or directory", args.source.display());
    };
    let stage_parent = parent_of(source_frames);

    let mut sources = sorted_entries(source_frames)?;
    ensure!(
        !sources.is_empty(),
        "{} has no frames",
        source_frames.display()
    );
    if args.mode == Mode::Cycle {
        sources.push(sources[0].clone());
    }
    let mut frames = Stage::new(stage_parent)?;
    frames.copy_from(&sources)?;

    if let Some(scale) = args.scale.filter(|scale| *scale != 1.0) {
        eprintln!("Scaling frames");
        let scaled = Stage::new(stage_parent)?;
        for source in frames.paths()? {
            let dest = scaled.path().join(file_name(&source)?);
            vips_resize(&source, &dest, scale)?;
        }
        frames = scaled;
    }

    if args.rife_iterations > 0 {
        eprintln!("Running rife");
        let interpolated = Stage::new(stage_parent)?;
        rife::interpolate(
            frames.path(),
            interpolated.path(),
            &args.rife_model,
            args.rife_iterations,
        )?;
        frames = interpolated;
    }

    if let Some(group) = args.blend.filter(|group| group.get() > 1) {
        eprintln!("Blending frames");
        let blended = Stage::new(stage_parent)?;
        for (index, chunk) in frames.paths()?.chunks(group.get()).enumerate() {
            let dest = blended.path().join(frame_name(index * group.get()));
            magick_mean(chunk, &dest)?;
        }
        frames = blended;
    }

    match args.mode {
        Mode::Bounce => {
            let bounced = Stage::new(stage_parent)?;
            bounced.copy_from(&bounce(frames.paths()?))?;
            frames = bounced;
        }
        Mode::Cycle => {
            // The appended first frame only existed to interpolate towards.
            if let Some(last) = frames.paths()?.pop() {
                fs::remove_file(&last).with_context(|| format!("removing {}", last.display()))?;
            }
        }
        Mode::Oneshot => {}
    }

    match args.format {
        Format::Mp4 | Format::Webm | Format::Webp => {
            eprintln!("Rendering video");
            // webp loops forever through `-loop 0`; repeating its frames only bloats the file.
            let loops = if args.format == Format::Webp {
                0
            } else {
                args.loops
            };
            let temp = ffmpeg::temp_beside(&output)?;
            ffmpeg::output(
                ffmpeg::command()
                    .arg("-stream_loop")
                    .arg(loops.to_string())
                    .arg("-framerate")
                    .arg(&args.fps)
                    .args(["-pattern_type", "glob", "-i"])
                    .arg(frames.path().join("*.png"))
                    .args(args.format.codec_args().unwrap_or_default())
                    .arg("-y")
                    .arg(&temp)
                    .stdout(Stdio::null()),
            )?;
            temp.persist(&output)
                .with_context(|| format!("writing {}", output.display()))?;
        }
        Format::Gif => {
            eprintln!("Rendering gif");
            let temp = ffmpeg::temp_beside(&output)?;
            gifski(&frames.paths()?, &temp, &args.fps)?;
            temp.persist(&output)
                .with_context(|| format!("writing {}", output.display()))?;
        }
        Format::Png => {
            fs::create_dir(&output).with_context(|| format!("creating {}", output.display()))?;
            for source in frames.paths()? {
                let dest = output.join(file_name(&source)?);
                fs::copy(&source, &dest).with_context(|| {
                    format!("copying {} to {}", source.display(), dest.display())
                })?;
            }
        }
    }
    println!("{}", output.display());
    Ok(())
}

/// A name in the working directory from the current local time.
fn default_output(format: Format) -> PathBuf {
    let mut name = jiff::Zoned::now().strftime("%Y%m%d_%H%M%S").to_string();
    if let Some(extension) = format.extension() {
        name.push('.');
        name.push_str(extension);
    }
    PathBuf::from(name)
}

fn parent_of(path: &Path) -> &Path {
    path.parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn file_name(path: &Path) -> Result<&std::ffi::OsStr> {
    path.file_name()
        .with_context(|| format!("{} has no file name", path.display()))
}

fn frame_name(index: usize) -> String {
    format!("{index:08}.png")
}

/// The entries of `dir` in name order.
fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("reading {}", dir.display()))?;
    paths.sort();
    Ok(paths)
}

/// The frames forward then backward, without repeating either end.
fn bounce(forward: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut frames = forward.clone();
    let inner = forward.len().saturating_sub(1);
    frames.extend(forward.into_iter().take(inner).skip(1).rev());
    frames
}

/// A temporary directory of frames, one stage of the pipeline.
struct Stage {
    dir: TempDir,
}

impl Stage {
    fn new(parent: &Path) -> Result<Self> {
        let dir = Builder::new()
            .prefix(TEMP_PREFIX)
            .tempdir_in(parent)
            .with_context(|| format!("creating a temporary directory in {}", parent.display()))?;
        Ok(Stage { dir })
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn paths(&self) -> Result<Vec<PathBuf>> {
        sorted_entries(self.path())
    }

    /// Copies `sources` in as zero-padded frames in order.
    fn copy_from(&self, sources: &[PathBuf]) -> Result<()> {
        for (index, source) in sources.iter().enumerate() {
            let dest = self.path().join(frame_name(index));
            fs::copy(source, &dest)
                .with_context(|| format!("copying {} to {}", source.display(), dest.display()))?;
        }
        Ok(())
    }
}

/// Writes the frames of `video` as PNGs into `dir`, dropping near-duplicate frames and
/// retiming the rest to the video's average frame rate.
fn extract_frames(video: &Path, dir: &Path, start: Option<&str>, end: Option<&str>) -> Result<()> {
    let frame_rate = ffmpeg::video_frame_rate(video)?;
    let mut command = ffmpeg::command();
    if let Some(start) = start {
        command.args(["-ss", start]);
    }
    if let Some(end) = end {
        command.args(["-to", end]);
    }
    command
        .arg("-i")
        .arg(video)
        .args(["-fps_mode", "passthrough", "-vf"])
        .arg(format!("mpdecimate,setpts=N/({frame_rate})/TB"))
        .args(["-compression_level", "0"])
        .arg(dir.join("%08d.png"))
        .stdout(Stdio::null());
    ffmpeg::output(&mut command)?;
    Ok(())
}

fn run_checked(command: &mut Command) -> Result<()> {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("running {program}"))?;
    if !status.success() {
        bail!("{program} failed: {status}");
    }
    Ok(())
}

fn vips_resize(source: &Path, dest: &Path, scale: f64) -> Result<()> {
    run_checked(
        Command::new("vips")
            .arg("resize")
            .arg(source)
            .arg(dest)
            .arg(scale.to_string()),
    )
}

/// The width in pixels of the image at `path`.
fn vips_width(path: &Path) -> Result<u32> {
    let output = Command::new("vipsheader")
        .args(["-f", "width", "--"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .context("running vipsheader")?;
    if !output.status.success() {
        bail!("vipsheader failed on {}: {}", path.display(), output.status);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim()
        .parse()
        .with_context(|| format!("parsing the width {:?} of {}", text.trim(), path.display()))
}

/// Averages `frames` into one image at `dest`. Done per group with ImageMagick rather than
/// ffmpeg's temporal filters, which miscount frames whose PNGs share a timestamp.
fn magick_mean(frames: &[PathBuf], dest: &Path) -> Result<()> {
    run_checked(
        Command::new("magick")
            .args(frames)
            .args(["-evaluate-sequence", "Mean"])
            .arg(dest),
    )
}

fn gifski(frames: &[PathBuf], dest: &Path, fps: &str) -> Result<()> {
    let Some(first) = frames.first() else {
        bail!("no frames to render");
    };
    // gifski caps animations near 800x600 unless told the width, so the frames keep theirs.
    let width = vips_width(first)?;
    run_checked(
        Command::new("gifski")
            .args(["--quiet", "--extra", "--width"])
            .arg(width.to_string())
            .args(["--fps", fps, "--output"])
            .arg(dest)
            .arg("--")
            .args(frames)
            .stdout(Stdio::null()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &str) -> Vec<PathBuf> {
        names
            .chars()
            .map(|name| PathBuf::from(name.to_string()))
            .collect()
    }

    #[test]
    fn bounce_mirrors_the_inner_frames() {
        assert_eq!(bounce(paths("abcd")), paths("abcdcb"));
    }

    #[test]
    fn bounce_of_two_frames_is_unchanged() {
        assert_eq!(bounce(paths("ab")), paths("ab"));
    }

    #[test]
    fn bounce_of_one_frame_is_unchanged() {
        assert_eq!(bounce(paths("a")), paths("a"));
    }

    #[test]
    fn frame_names_sort_in_order() {
        assert!(frame_name(9) < frame_name(10));
    }

    #[test]
    fn default_output_takes_the_format_extension() {
        assert!(
            default_output(Format::Webm)
                .to_string_lossy()
                .ends_with(".webm")
        );
        assert!(default_output(Format::Png).extension().is_none());
    }
}
