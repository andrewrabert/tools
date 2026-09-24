use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;

use anyhow::{Context, Error, Result, bail};

use crate::tools::archive::extract::backend::{Request, TEMP_PREFIX, file_name};
use crate::tools::archive::extract::listing::Contents;
use crate::tools::archive::process;
use crate::tools::archive::temp;

const EXTENSION: &str = ".cue";
const CHUNK_PREFIX: &str = "chunk";
const MERGED_NAME: &str = "merged";

enum TrackType {
    Audio,
    Mode1,
    Mode2,
}

impl TrackType {
    fn suffix(&self) -> &'static str {
        match self {
            TrackType::Audio => ".wav",
            TrackType::Mode1 | TrackType::Mode2 => ".iso",
        }
    }
}

impl FromStr for TrackType {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        match text {
            "AUDIO" => Ok(TrackType::Audio),
            "MODE1/2352" => Ok(TrackType::Mode1),
            "MODE2/2352" => Ok(TrackType::Mode2),
            other => bail!("unsupported track type {other}"),
        }
    }
}

struct Track {
    file: PathBuf,
    track_type: TrackType,
}

fn tracks(cue: &Path) -> Result<Vec<Track>> {
    let sheet = fs::read_to_string(cue).with_context(|| format!("reading {}", cue.display()))?;
    let parent = cue
        .parent()
        .with_context(|| format!("{} has no parent", cue.display()))?;
    let mut tracks = Vec::new();
    let mut current_file = None;
    for line in sheet.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("FILE ") {
            let name = rest
                .trim_start()
                .rsplit_once(char::is_whitespace)
                .map_or(rest, |(name, _file_type)| name)
                .trim();
            let name = name.strip_prefix('"').unwrap_or(name);
            let name = name.strip_suffix('"').unwrap_or(name);
            let file = parent.join(name);
            if !file.is_file() {
                bail!("{} named in the cue sheet does not exist", file.display());
            }
            current_file = Some(file);
        } else if let (Some(file), true) = (&current_file, line.starts_with("TRACK ")) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let &[_, _, track_type] = fields.as_slice() else {
                bail!("unexpected cue sheet line: {line}");
            };
            tracks.push(Track {
                file: file.clone(),
                track_type: track_type.parse()?,
            });
        }
    }
    if tracks.is_empty() {
        bail!("{} has no tracks", cue.display());
    }
    Ok(tracks)
}

fn content_name(cue: &Path) -> Result<String> {
    let name = file_name(cue)?
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", cue.display()))?;
    let stem_length = name.len().saturating_sub(EXTENSION.len());
    match name.split_at_checked(stem_length) {
        Some((stem, extension))
            if !stem.is_empty() && extension.eq_ignore_ascii_case(EXTENSION) =>
        {
            Ok(stem.to_owned())
        }
        _ => bail!("{} must end with {EXTENSION}", cue.display()),
    }
}

pub fn contents(cue: &Path) -> Result<Contents> {
    let name = content_name(cue)?;
    let tracks = tracks(cue)?;
    let numbered = tracks.len() > 1;
    Ok(Contents::named(tracks.iter().enumerate().map(
        |(index, track)| {
            let suffix = track.track_type.suffix();
            if numbered {
                format!("{name}{:02}{suffix}", index + 1).into_bytes()
            } else {
                format!("{name}{suffix}").into_bytes()
            }
        },
    )))
}

fn data_files(cue: &Path) -> Result<BTreeSet<PathBuf>> {
    Ok(tracks(cue)?.into_iter().map(|track| track.file).collect())
}

pub fn volumes(cue: &Path) -> Result<Vec<PathBuf>> {
    let mut volumes = vec![cue.to_owned()];
    volumes.extend(data_files(cue)?);
    Ok(volumes)
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    request.keeping_existing()?;
    request.without_password()?;
    let directory = request.directory()?;
    let cue = request.archive;
    let name = content_name(cue)?;
    let data_files = data_files(cue)?;
    let scratch = temp::dir(directory, file_name(directory)?, TEMP_PREFIX)?;

    let (source_bin, source_cue) = match (data_files.first(), data_files.len()) {
        (Some(data_file), 1) => (data_file.clone(), cue.to_owned()),
        _ => {
            process::run(
                Command::new("binmerge")
                    .arg("--outdir")
                    .arg(scratch.path())
                    .arg(cue)
                    .arg(MERGED_NAME),
            )?;
            (
                PathBuf::from(format!("{MERGED_NAME}.bin")),
                PathBuf::from(format!("{MERGED_NAME}.cue")),
            )
        }
    };
    process::run(
        Command::new("bchunk")
            .arg("-w")
            .arg(source_bin)
            .arg(source_cue)
            .arg(CHUNK_PREFIX)
            .current_dir(scratch.path())
            .stdout(Stdio::null()),
    )?;

    let mut produced = Vec::new();
    for entry in fs::read_dir(scratch.path())
        .with_context(|| format!("listing {}", scratch.path().display()))?
    {
        let entry = entry.with_context(|| format!("listing {}", scratch.path().display()))?;
        produced.push(entry.path());
    }

    let mut moves = Vec::new();
    for source in &produced {
        let chunk_name = file_name(source)?.to_string_lossy();
        let mut target_name = OsString::from(&name);
        if produced.len() == 1 {
            if let Some(extension) = source.extension() {
                target_name.push(".");
                target_name.push(extension);
            }
        } else if let Some(numbered) = chunk_name.strip_prefix(CHUNK_PREFIX) {
            target_name.push(numbered);
        } else {
            continue;
        }
        let target = directory.join(target_name);
        if target.exists() {
            bail!("{} already exists", target.display());
        }
        moves.push((source, target));
    }
    for (source, target) in moves {
        fs::rename(source, &target).with_context(|| format!("moving to {}", target.display()))?;
    }
    Ok(())
}
