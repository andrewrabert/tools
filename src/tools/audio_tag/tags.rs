use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result, bail};
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::flac::FlacFile;
use lofty::ogg::tag::VorbisComments;
use lofty::ogg::{OpusFile, SpeexFile, VorbisFile};
use lofty::probe::Probe;
use lofty::tag::{ItemKey, ItemValue, Tag, TagExt, TagItem, TagType};

const AUDIO_EXTS: [&str; 8] = ["aac", "ape", "flac", "m4a", "mp3", "ogg", "opus", "wav"];

/// Tags listed first, in this order; the rest follow alphabetically.
const PRIORITY: [&str; 6] = [
    "title",
    "tracknumber",
    "artist",
    "album",
    "genre",
    "albumartist",
];

pub fn is_audio(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| AUDIO_EXTS.contains(&extension.to_lowercase().as_str()))
}

/// A track's text tags, keyed by lowercase name, in display order.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Tags(Vec<(String, Vec<String>)>);

impl Tags {
    pub fn read(path: &Path) -> Result<Self> {
        let tags = match Store::read(path)? {
            Store::Vorbis(comments) => {
                let mut tags = Tags::default();
                for (key, value) in comments.items() {
                    tags.push(key.to_lowercase(), value.to_owned());
                }
                tags
            }
            Store::Generic(tag) => {
                let mut tags = Tags::default();
                for item in tag.items() {
                    if let ItemValue::Text(value) | ItemValue::Locator(value) = item.value() {
                        tags.push(key_name(item.key(), tag.tag_type()), value.clone());
                    }
                }
                tags
            }
        };
        Ok(tags.sorted())
    }

    /// Replace every text tag in the file with these.
    pub fn write(&self, path: &Path) -> Result<()> {
        let options = WriteOptions::default();
        match Store::read(path)? {
            Store::Vorbis(mut comments) => {
                drop(comments.take_items());
                for (name, values) in &self.0 {
                    for value in values {
                        comments.push(name.to_uppercase(), value.clone());
                    }
                }
                comments
                    .save_to_path(path, options)
                    .with_context(|| format!("writing tags to {}", path.display()))?;
            }
            Store::Generic(mut tag) => {
                let tag_type = tag.tag_type();
                tag.retain(|item| {
                    !matches!(item.value(), ItemValue::Text(_) | ItemValue::Locator(_))
                });
                for (name, values) in &self.0 {
                    let Some(key) = item_key(name, tag_type) else {
                        bail!("{name} is not supported in {tag_type:?} tags");
                    };
                    for value in values {
                        if !tag.push(TagItem::new(key, ItemValue::Text(value.clone()))) {
                            bail!("{name} is not supported in {tag_type:?} tags");
                        }
                    }
                }
                tag.save_to_path(path, options)
                    .with_context(|| format!("writing tags to {}", path.display()))?;
            }
        }
        Ok(())
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &[String])> {
        self.0
            .iter()
            .map(|(name, values)| (name.as_str(), values.as_slice()))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&[String]> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, values)| values.as_slice())
    }

    /// Replace the values of `name`, keeping its position, or append it.
    pub fn set(&mut self, name: &str, values: Vec<String>) {
        match self.0.iter_mut().find(|(key, _)| key == name) {
            Some((_, existing)) => *existing = values,
            None => self.0.push((name.to_owned(), values)),
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<Vec<String>> {
        let index = self.0.iter().position(|(key, _)| key == name)?;
        Some(self.0.remove(index).1)
    }

    /// Priority tags first in their order, then the rest alphabetically.
    pub fn sorted(mut self) -> Self {
        self.0.sort_by_key(|(name, _)| {
            let priority = PRIORITY.iter().position(|key| key == name);
            (priority.unwrap_or(PRIORITY.len()), name.clone())
        });
        self
    }

    /// Tags by name, for comparison regardless of order.
    pub fn by_name(&self) -> BTreeMap<&str, &[String]> {
        self.iter().collect()
    }

    fn push(&mut self, name: String, value: String) {
        match self.0.iter_mut().find(|(key, _)| *key == name) {
            Some((_, values)) => values.push(value),
            None => self.0.push((name, vec![value])),
        }
    }
}

impl FromIterator<(String, Vec<String>)> for Tags {
    fn from_iter<I: IntoIterator<Item = (String, Vec<String>)>>(iter: I) -> Self {
        let mut tags = Tags::default();
        for (name, values) in iter {
            tags.set(&name, values);
        }
        tags
    }
}

/// Vorbis comments are kept raw so every key survives; other formats go through lofty's
/// generic tag, which keeps unmapped items aside and restores them on save.
enum Store {
    Vorbis(VorbisComments),
    Generic(Tag),
}

impl Store {
    fn read(path: &Path) -> Result<Self> {
        let opening = || format!("opening {}", path.display());
        let reading = || format!("reading tags from {}", path.display());
        let probe = Probe::open(path)
            .with_context(opening)?
            .guess_file_type()
            .with_context(opening)?;
        match probe.file_type() {
            Some(
                file_type @ (FileType::Flac | FileType::Opus | FileType::Vorbis | FileType::Speex),
            ) => {
                let mut file = File::open(path).with_context(opening)?;
                let comments = read_vorbis(&mut file, file_type).with_context(reading)?;
                Ok(Store::Vorbis(comments))
            }
            _ => {
                let tagged = probe.read().with_context(reading)?;
                let tag = match tagged.primary_tag() {
                    Some(tag) => tag.clone(),
                    None => Tag::new(tagged.primary_tag_type()),
                };
                Ok(Store::Generic(tag))
            }
        }
    }
}

fn read_vorbis(file: &mut File, file_type: FileType) -> Result<VorbisComments> {
    let options = ParseOptions::default();
    let comments = match file_type {
        FileType::Flac => FlacFile::read_from(file, options)?
            .vorbis_comments()
            .cloned()
            .unwrap_or_default(),
        FileType::Opus => OpusFile::read_from(file, options)?
            .vorbis_comments()
            .clone(),
        FileType::Vorbis => VorbisFile::read_from(file, options)?
            .vorbis_comments()
            .clone(),
        FileType::Speex => SpeexFile::read_from(file, options)?
            .vorbis_comments()
            .clone(),
        _ => bail!("{file_type:?} files do not hold Vorbis comments"),
    };
    Ok(comments)
}

/// Vorbis comment names where one exists, so every format shares the same names.
fn key_name(key: ItemKey, tag_type: TagType) -> String {
    key.map_key(TagType::VorbisComments)
        .or_else(|| key.map_key(tag_type))
        .map_or_else(|| format!("{key:?}"), str::to_owned)
        .to_lowercase()
}

fn item_key(name: &str, tag_type: TagType) -> Option<ItemKey> {
    ItemKey::from_key(TagType::VorbisComments, &name.to_uppercase())
        .or_else(|| ItemKey::from_key(tag_type, &name.to_uppercase()))
        .or_else(|| ItemKey::from_key(tag_type, name))
}
