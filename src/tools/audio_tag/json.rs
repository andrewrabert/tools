use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::{Map, Value};

use crate::source::Source;
use crate::tools::audio_tag::tags::Tags;

/// A tag's change: new values, or deletion (`null` in the JSON).
#[derive(Clone)]
pub enum Update {
    Set(Vec<String>),
    Delete,
}

/// Tag changes read from JSON, per file.
pub struct FileUpdates {
    pub path: PathBuf,
    pub updates: Vec<(String, Update)>,
}

/// A single value as a string, several as a list.
pub struct Values<'a>(pub &'a [String]);

impl Serialize for Values<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            [value] => serializer.serialize_str(value),
            values => values.serialize(serializer),
        }
    }
}

impl Serialize for Tags {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (name, values) in self.iter() {
            map.serialize_entry(name, &Values(values))?;
        }
        map.end()
    }
}

/// Entries serialized as a JSON object in slice order.
pub struct Ordered<'a, V>(pub &'a [(String, V)]);

impl<V: Serialize> Serialize for Ordered<'_, V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// Write `value` as indented JSON to `output`, or to stdout.
pub fn write(value: &impl Serialize, output: Option<&Path>) -> Result<()> {
    match output {
        Some(path) => {
            let mut file = std::fs::File::create(path)
                .with_context(|| format!("creating {}", path.display()))?;
            serde_json::to_writer_pretty(&mut file, value)?;
        }
        None => {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer_pretty(&mut stdout, value)?;
            writeln!(stdout)?;
        }
    }
    Ok(())
}

/// Read `{path: {tag: value}}`, or the structured form, from `source`.
pub fn read(source: &Source, structured: bool) -> Result<Vec<FileUpdates>> {
    let value: Value =
        serde_json::from_reader(BufReader::new(source.open()?)).context("parsing input JSON")?;
    let files = if structured {
        expand_structured(value)?
    } else {
        object(value, "input")?
    };
    files
        .into_iter()
        .map(|(path, tags)| {
            let updates = object(tags, &path)?
                .into_iter()
                .map(|(name, value)| Ok((name.to_lowercase(), update(value, &path)?)))
                .collect::<Result<_>>()?;
            Ok(FileUpdates {
                path: PathBuf::from(path),
                updates,
            })
        })
        .collect()
}

/// Flatten `{dir/: {album, albumartist, date, genre, tracks: {file: tags}}}` to `{path: tags}`.
fn expand_structured(value: Value) -> Result<Map<String, Value>> {
    let mut flat = Map::new();
    for (dir, entry) in object(value, "input")? {
        let mut entry = object(entry, &dir)?;
        let shared = |entry: &mut Map<String, Value>, key: &str| entry.remove(key).filter(truthy);
        let album = shared(&mut entry, "album");
        let albumartist = shared(&mut entry, "albumartist");
        let date = shared(&mut entry, "date");
        let genre = shared(&mut entry, "genre");
        let tracks = match entry.remove("tracks") {
            Some(tracks) => object(tracks, &dir)?,
            None => Map::new(),
        };
        for (filename, tags) in tracks {
            let path = Path::new(&dir)
                .join(&filename)
                .to_string_lossy()
                .into_owned();
            let mut tags = object(tags, &path)?;
            if let Some(album) = &album {
                tags.insert("album".to_owned(), album.clone());
            }
            if let Some(albumartist) = &albumartist {
                tags.insert("albumartist".to_owned(), albumartist.clone());
                tags.entry("artist").or_insert_with(|| albumartist.clone());
            }
            if let Some(date) = &date {
                tags.insert("date".to_owned(), date.clone());
            }
            if let Some(genre) = &genre {
                tags.entry("genre").or_insert_with(|| genre.clone());
            }
            flat.insert(path, Value::Object(tags));
        }
    }
    Ok(flat)
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => !value.is_empty(),
        Value::Array(values) => !values.is_empty(),
        _ => true,
    }
}

fn object(value: Value, what: &str) -> Result<Map<String, Value>> {
    match value {
        Value::Object(map) => Ok(map),
        _ => bail!("{what}: expected a JSON object"),
    }
}

fn update(value: Value, path: &str) -> Result<Update> {
    Ok(match value {
        Value::Null => Update::Delete,
        Value::String(value) => Update::Set(vec![value]),
        Value::Array(values) => Update::Set(
            values
                .into_iter()
                .map(|value| match value {
                    Value::String(value) => Ok(value),
                    _ => bail!("{path}: tag values must be strings"),
                })
                .collect::<Result<_>>()?,
        ),
        _ => bail!("{path}: tag values must be strings, lists of strings, or null"),
    })
}
