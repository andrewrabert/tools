use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{self, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

use crate::tools::audio_tag::json::{self, Ordered, Values};
use crate::tools::audio_tag::tags::{self, Tags};
use crate::walk;

/// Tags every track in a directory shares, lifted out of the tracks.
const SHARED: [&str; 4] = ["album", "albumartist", "date", "genre"];

/// A directory in the structured dump.
struct Dir {
    album: Option<Vec<String>>,
    albumartist: Option<Vec<String>>,
    date: Option<Vec<String>>,
    genre: Option<Vec<String>>,
    tracks: Vec<(String, Tags)>,
}

impl Serialize for Dir {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        let shared = [&self.album, &self.albumartist, &self.date, &self.genre];
        for (name, values) in SHARED.iter().zip(shared) {
            if let Some(values) = values {
                map.serialize_entry(name, &Values(values))?;
            }
        }
        map.serialize_entry("tracks", &Ordered(&self.tracks))?;
        map.end()
    }
}

pub fn dump(path: &Path, structured: bool, output: Option<&Path>) -> Result<()> {
    let path = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
    if path.is_dir() {
        if structured {
            let dirs = structured_tags(&path)?;
            json::write(&dirs, output)?;
            if let Some(output) = output {
                eprintln!(
                    "\nDumped {} directories to {}",
                    dirs.len(),
                    output.display()
                );
            }
        } else {
            let files = audio_files(&path)?
                .into_iter()
                .map(|file| Ok((file.to_string_lossy().into_owned(), Tags::read(&file)?)))
                .collect::<Result<BTreeMap<_, _>>>()?;
            json::write(&files, output)?;
            if let Some(output) = output {
                eprintln!("\nDumped {} files to {}", files.len(), output.display());
            }
        }
    } else if path.is_file() {
        if structured {
            bail!("structured mode requires a directory");
        }
        let file = BTreeMap::from([(path.to_string_lossy().into_owned(), Tags::read(&path)?)]);
        json::write(&file, output)?;
        if let Some(output) = output {
            eprintln!(
                "Dumped tags from {} to {}",
                path.display(),
                output.display()
            );
        }
    } else if !path.exists() {
        bail!("path does not exist: {}", path.display());
    } else {
        bail!("invalid path: {}", path.display());
    }
    Ok(())
}

/// Audio files under `root`, sorted by directory, then name.
pub fn audio_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in walk::entries(root) {
        let path = entry?.into_path();
        if tags::is_audio(&path) {
            files.push(path);
        }
    }
    files.sort_by(|a, b| (a.parent(), a.file_name()).cmp(&(b.parent(), b.file_name())));
    Ok(files)
}

fn structured_tags(root: &Path) -> Result<BTreeMap<String, Dir>> {
    let mut dirs: BTreeMap<PathBuf, Vec<(String, Tags)>> = BTreeMap::new();
    for path in audio_files(root)? {
        let tags = Tags::read(&path)?;
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            continue;
        };
        dirs.entry(parent.to_path_buf())
            .or_default()
            .push((name.to_string_lossy().into_owned(), tags));
    }
    dirs.into_iter()
        .map(|(dir, files)| {
            let entry = dir_entry(&dir, files)?;
            Ok((format!("{}/", dir.display()), entry))
        })
        .collect()
}

fn dir_entry(dir: &Path, files: Vec<(String, Tags)>) -> Result<Dir> {
    let mut shared: [BTreeSet<Vec<String>>; 4] = Default::default();
    let mut tracknumbers: HashMap<&[String], &str> = HashMap::new();
    for (name, tags) in &files {
        for (values, key) in shared.iter_mut().zip(SHARED) {
            if let Some(value) = tags.get(key) {
                values.insert(value.to_vec());
            }
        }
        if let Some(tracknumber) = tags.get("tracknumber")
            && let Some(previous) = tracknumbers.insert(tracknumber, name)
        {
            bail!(
                "duplicate tracknumber '{}' in {}: {previous} and {name}",
                tracknumber.join("; "),
                dir.display()
            );
        }
    }
    let [albums, albumartists, dates, genres] = shared;
    let album = only(albums, "albums", dir)?;
    let albumartist = only(albumartists, "albumartists", dir)?;
    let date = only(dates, "dates", dir)?;
    let genre = (genres.len() == 1)
        .then(|| genres.into_iter().next())
        .flatten();

    let mut tracks: Vec<(String, Tags)> = files
        .into_iter()
        .map(|(name, mut tags)| {
            tags.remove("album");
            tags.remove("albumartist");
            tags.remove("date");
            if genre.is_some() {
                tags.remove("genre");
            }
            if tags.get("artist") == albumartist.as_deref() {
                tags.remove("artist");
            }
            (name, tags)
        })
        .collect();
    tracks.sort_by_cached_key(|(name, tags)| (track_order(tags), name.clone()));
    Ok(Dir {
        album,
        albumartist,
        date,
        genre,
        tracks,
    })
}

fn only(values: BTreeSet<Vec<String>>, what: &str, dir: &Path) -> Result<Option<Vec<String>>> {
    if values.len() > 1 {
        bail!("differing {what} in {}: {values:?}", dir.display());
    }
    Ok(values.into_iter().next())
}

/// Numeric track numbers in order, a missing one first, anything else last.
fn track_order(tags: &Tags) -> u128 {
    match tags.get("tracknumber") {
        None => 0,
        Some([number])
            if !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            number.parse().unwrap_or(u128::MAX)
        }
        Some(_) => u128::MAX,
    }
}
