use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use clap::Args as ClapArgs;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::mkvmerge::{Identify, TrackType};
use crate::tools::Tool;

const TRACK_TYPES: [TrackType; 3] = [TrackType::Video, TrackType::Audio, TrackType::Subtitles];
const SIZE_UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
/// Shown in place of a language code when a track has none.
const NO_LANGUAGE: &str = "(none)";

#[derive(ClapArgs)]
#[command(about = "Summarize the tracks of a Matroska file")]
pub struct MkvPreview {
    #[arg(value_name = "PATH")]
    path: PathBuf,
}

impl Tool for MkvPreview {
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

/// The output of `mediainfo --Output=JSON`. Every value is a string.
#[derive(Deserialize)]
struct MediaInfo {
    media: Media,
}

#[derive(Deserialize)]
struct Media {
    track: Vec<MediaTrack>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MediaTrack {
    #[serde(rename = "UniqueID")]
    unique_id: Option<String>,
    format: Option<String>,
    bit_depth: Option<String>,
    width: Option<String>,
    height: Option<String>,
}

fn run(args: MkvPreview) -> Result<()> {
    let output = summary(&args.path)?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(&output)?;
    stdout.flush()?;
    Ok(())
}

pub fn summary(path: &Path) -> Result<Vec<u8>> {
    // Both are spawned before either is awaited so they run concurrently.
    let mkvmerge = spawn("mkvmerge", Command::new("mkvmerge").arg("-J").arg(path))?;
    let mediainfo = spawn(
        "mediainfo",
        Command::new("mediainfo")
            .args(["--Output=JSON", "--"])
            .arg(path),
    )?;
    let mkvmerge: Identify = finish("mkvmerge", mkvmerge)?;
    let mediainfo: MediaInfo = finish("mediainfo", mediainfo)?;

    let mediainfo_by_uid = mediainfo_by_uid(&mediainfo)?;
    let mut sections: [Vec<String>; TRACK_TYPES.len()] = Default::default();
    for track in &mkvmerge.tracks {
        let Some(index) = TRACK_TYPES.iter().position(|kind| *kind == track.kind) else {
            continue;
        };
        let selector = format!("track:{}{}", short(track.kind), sections[index].len() + 1);
        let mut fields = vec![format!("{} {selector}", track.id)];
        let properties = &track.properties;
        if track.kind == TrackType::Video {
            let uid = properties
                .uid
                .with_context(|| format!("{selector} has no uid"))?;
            let info = mediainfo_by_uid
                .get(&u128::from(uid))
                .ok_or_else(|| anyhow!("mediainfo has no track with uid {uid}"))?;
            fields.push(video_format(info)?);
            fields.push(format!(
                "{}x{}",
                dimension(info.width.as_deref(), "width", uid)?,
                dimension(info.height.as_deref(), "height", uid)?,
            ));
        } else {
            let language = properties.language.as_deref().unwrap_or(NO_LANGUAGE);
            fields.push(language.to_owned());
            fields.push(track.codec.clone());
            if let Some(name) = &properties.track_name {
                fields.push(name.clone());
            }
        }
        sections[index].push(fields.join("\t"));
    }

    let size = fs::metadata(path)
        .with_context(|| format!("reading the size of {}", path.display()))?
        .len();
    let container = &mkvmerge.container.properties;
    let mut out = Vec::new();
    if let Some(title) = &container.title {
        writeln!(out, "title: {}", title.trim())?;
    }
    if let Some(nanoseconds) = container.duration {
        writeln!(out, "duration: {}", human_duration(nanoseconds))?;
    }
    writeln!(out, "size: {}", human_size(size))?;
    for (kind, lines) in TRACK_TYPES.iter().zip(&sections) {
        if lines.is_empty() {
            continue;
        }
        writeln!(out, "{}", heading(*kind))?;
        for line in lines {
            writeln!(out, "  {line}")?;
        }
    }
    Ok(out)
}

fn heading(kind: TrackType) -> &'static str {
    match kind {
        TrackType::Video => "video",
        TrackType::Audio => "audio",
        TrackType::Subtitles => "subtitles",
        TrackType::Other => "other",
    }
}

fn short(kind: TrackType) -> &'static str {
    &heading(kind)[..1]
}

fn spawn(name: &str, command: &mut Command) -> Result<Child> {
    command
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {name}"))
}

fn finish<T: DeserializeOwned>(name: &str, child: Child) -> Result<T> {
    let output = child
        .wait_with_output()
        .with_context(|| format!("waiting for {name}"))?;
    if !output.status.success() {
        bail!("{name} failed: {}", output.status);
    }
    serde_json::from_slice(&output.stdout).with_context(|| format!("parsing the output of {name}"))
}

// The general track's UniqueID is the 128-bit segment uid, so u64 is too narrow.
fn mediainfo_by_uid(mediainfo: &MediaInfo) -> Result<HashMap<u128, &MediaTrack>> {
    let mut by_uid = HashMap::new();
    for track in &mediainfo.media.track {
        let Some(uid) = &track.unique_id else {
            continue;
        };
        let uid: u128 = uid
            .trim()
            .parse()
            .with_context(|| format!("parsing the mediainfo uid {uid:?}"))?;
        if by_uid.insert(uid, track).is_some() {
            bail!("mediainfo reported the uid {uid} twice");
        }
    }
    Ok(by_uid)
}

fn dimension<'a>(value: Option<&'a str>, name: &str, uid: u64) -> Result<&'a str> {
    value.with_context(|| format!("mediainfo reported no {name} for the track with uid {uid}"))
}

fn video_format(info: &MediaTrack) -> Result<String> {
    let format = info
        .format
        .as_deref()
        .context("mediainfo reported no video format")?;
    Ok(match format {
        "AVC" => "AVC (H.264)".to_owned(),
        "HEVC" => match &info.bit_depth {
            None => "HEVC (H.265)".to_owned(),
            Some(depth) => format!("HEVC (H.265) ({depth}-bit)"),
        },
        other => other.to_owned(),
    })
}

// Matches the `H:MM:SS[.ffffff]` rendering of Python's timedelta.
fn human_duration(nanoseconds: u64) -> String {
    let total_micros = (nanoseconds + 500) / 1000;
    let micros = total_micros % 1_000_000;
    let total_seconds = total_micros / 1_000_000;
    let days = total_seconds / 86_400;
    let hours = total_seconds % 86_400 / 3600;
    let minutes = total_seconds % 3600 / 60;
    let seconds = total_seconds % 60;

    let mut duration = match days {
        0 => String::new(),
        1 => "1 day, ".to_owned(),
        days => format!("{days} days, "),
    };
    duration.push_str(&format!("{hours}:{minutes:02}:{seconds:02}"));
    if micros != 0 {
        duration.push_str(&format!(".{micros:06}"));
    }
    duration
}

fn human_size(bytes: u64) -> String {
    let mut size = bytes as f64;
    let mut unit = SIZE_UNITS[0];
    for next in SIZE_UNITS.into_iter().skip(1) {
        if size < 1024.0 {
            break;
        }
        size /= 1024.0;
        unit = next;
    }
    format!("{size:.1}{unit}")
}
