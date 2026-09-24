use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use lofty::tag::TagType;
use serde_json::Value;

const M4A_COVER_CODECS: [&str; 2] = ["mjpeg", "png"];

#[derive(Clone, Copy)]
pub enum Source {
    Flac,
    Ape,
    M4a,
    Wv,
    Wav,
}

impl Source {
    pub fn of(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match extension.as_str() {
            "flac" => Source::Flac,
            "ape" => Source::Ape,
            "m4a" => Source::M4a,
            "wv" => Source::Wv,
            "wav" => Source::Wav,
            _ => return None,
        })
    }

    /// The process (or file) whose stdout is raw WAV.
    pub fn decoder(self, path: &Path, flac_decode_through_errors: bool) -> Result<Decoder> {
        let mut command = match self {
            Source::Flac => {
                let mut command = Command::new("flac");
                command.args(["--stdout", "--silent", "--decode"]);
                if flac_decode_through_errors {
                    command.arg("--decode-through-errors");
                }
                command.arg("--").arg(path);
                command
            }
            Source::Ape => {
                let mut command = Command::new("mac");
                command.arg(path).args(["-", "-d"]);
                command
            }
            Source::M4a => {
                check_alac(path)?;
                let mut command = Command::new("ffmpeg");
                command.arg("-i").arg(path).args([
                    "-f",
                    "wav",
                    "-acodec",
                    "pcm_s16le",
                    "-ac",
                    "2",
                    "-",
                ]);
                command
            }
            Source::Wv => {
                let mut command = Command::new("wvunpack");
                command.arg("-q").arg(path).args(["-o", "-"]);
                command
            }
            Source::Wav => {
                let file =
                    File::open(path).with_context(|| format!("opening {}", path.display()))?;
                return Ok(Decoder::File(file));
            }
        };
        command.stderr(Stdio::null());
        Ok(Decoder::Process(command))
    }
}

pub enum Decoder {
    Process(Command),
    File(File),
}

// An m4a is a container; only a lone stereo 16-bit ALAC track (plus cover art) is lossless audio.
fn check_alac(path: &Path) -> Result<()> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_streams",
            "-print_format",
            "json",
            "--",
        ])
        .arg(path)
        .stderr(Stdio::inherit())
        .output()
        .context("running ffprobe")?;
    if !output.status.success() {
        bail!("ffprobe failed: {}", output.status);
    }
    let info: Value =
        serde_json::from_slice(&output.stdout).context("parsing the output of ffprobe")?;
    let streams = info["streams"]
        .as_array()
        .context("ffprobe reported no streams")?;
    let mut alac = None;
    for stream in streams {
        let codec_type = stream["codec_type"].as_str().unwrap_or_default();
        let codec_name = stream["codec_name"].as_str().unwrap_or_default();
        match codec_type {
            "audio" if codec_name == "alac" => {
                if alac.replace(stream).is_some() {
                    bail!("multiple ALAC streams");
                }
            }
            "audio" => bail!("unhandled audio type: {codec_name}"),
            "video" if M4A_COVER_CODECS.contains(&codec_name) => {}
            "video" => bail!("unhandled video type: {codec_name}"),
            other => bail!("unhandled stream type: {other}"),
        }
    }
    let alac = alac.context("not ALAC file")?;
    if alac["sample_fmt"].as_str() != Some("s16p") {
        bail!("unhandled sample format");
    }
    if alac["channels"].as_u64() != Some(2) {
        bail!("unhandled channel count");
    }
    if alac["channel_layout"].as_str() != Some("stereo") {
        bail!("unhandled channel layout");
    }
    Ok(())
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Target {
    Flac,
    Wav,
}

impl Target {
    pub fn extension(self) -> &'static str {
        match self {
            Target::Flac => "flac",
            Target::Wav => "wav",
        }
    }

    pub fn tag_type(self) -> TagType {
        match self {
            Target::Flac => TagType::VorbisComments,
            Target::Wav => TagType::Id3v2,
        }
    }

    /// The process that reads raw WAV on stdin and writes `dest`, if one is needed.
    pub fn encoder(self, dest: &Path) -> Option<Command> {
        match self {
            Target::Flac => {
                let mut command = Command::new("flac");
                command
                    .args(["--silent", "--best", "--verify", "--force", "--output-name"])
                    .arg(dest)
                    .arg("-")
                    .stderr(Stdio::null());
                Some(command)
            }
            Target::Wav => None,
        }
    }
}
