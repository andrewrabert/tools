use std::collections::BTreeSet;
use std::path::{self, Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::walk;

pub const EXTENSION: &str = "mkv";

/// An ISO 639-2 language of a track.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Undetermined,
    /// E.g. instrumental.
    NonLinguistic,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Language::English => "eng",
            Language::Undetermined => "und",
            Language::NonLinguistic => "zxx",
        }
    }
}

/// The output of `mkvmerge -J`.
#[derive(Deserialize)]
pub struct Identify {
    pub container: Container,
    pub tracks: Vec<Track>,
}

#[derive(Deserialize)]
pub struct Container {
    #[serde(default)]
    pub properties: ContainerProperties,
}

#[derive(Default, Deserialize)]
pub struct ContainerProperties {
    pub title: Option<String>,
    /// Nanoseconds.
    pub duration: Option<u64>,
}

#[derive(Deserialize)]
pub struct Track {
    pub id: u64,
    pub codec: String,
    #[serde(rename = "type")]
    pub kind: TrackType,
    #[serde(default)]
    pub properties: TrackProperties,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackType {
    Video,
    Audio,
    Subtitles,
    #[serde(other)]
    Other,
}

#[derive(Default, Deserialize)]
pub struct TrackProperties {
    pub uid: Option<u64>,
    pub language: Option<String>,
    pub track_name: Option<String>,
}

impl Track {
    /// The ISO 639-2 language. An error when mkvmerge reported none.
    pub fn language(&self) -> Result<&str> {
        self.properties
            .language
            .as_deref()
            .with_context(|| format!("track {} has no language", self.id))
    }
}

pub fn identify(path: &Path) -> Result<Identify> {
    let output = Command::new("mkvmerge")
        .arg("-J")
        .arg(path)
        .stderr(Stdio::inherit())
        .output()
        .context("running mkvmerge")?;
    if !output.status.success() {
        bail!("mkvmerge failed: {}", output.status);
    }
    serde_json::from_slice(&output.stdout).context("parsing the output of mkvmerge")
}

/// The Matroska files among and under `paths`, absolute and sorted.
///
/// Absolute, since mkvmerge, mkvextract, and mkvpropedit accept no `--` to end their options.
pub fn find_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();
    for path in paths {
        let path = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        for entry in walk::entries(&path) {
            let entry = entry?;
            if entry.file_type().is_file() && is_mkv(entry.path()) {
                files.insert(entry.into_path());
            }
        }
    }
    Ok(files.into_iter().collect())
}

pub fn is_mkv(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION))
}
