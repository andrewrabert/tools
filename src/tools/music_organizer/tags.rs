use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result, bail};
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use lofty::id3::v2::Id3v2Tag;
use lofty::mpeg::MpegFile;
use lofty::ogg::OpusFile;
use lofty::ogg::tag::VorbisComments;
use lofty::tag::{ItemKey, ItemValue, MergeTag, SplitTag, Tag, TagExt, TagItem};
use unicode_normalization::UnicodeNormalization;

use crate::tools::music_organizer::name::{collapse_whitespace, is_printable};

pub const FLAC_EXT: &str = "flac";
pub const MP3_EXT: &str = "mp3";
pub const OPUS_EXT: &str = "opus";
pub const AUDIO_EXTS: [&str; 3] = [FLAC_EXT, MP3_EXT, OPUS_EXT];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Field {
    AlbumArtist,
    Album,
    Artist,
    Date,
    Genre,
    Title,
    TrackNumber,
}

impl Field {
    pub const EXPECTED: [Field; 7] = [
        Field::AlbumArtist,
        Field::Album,
        Field::Artist,
        Field::Date,
        Field::Genre,
        Field::Title,
        Field::TrackNumber,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Field::AlbumArtist => "albumartist",
            Field::Album => "album",
            Field::Artist => "artist",
            Field::Date => "date",
            Field::Genre => "genre",
            Field::Title => "title",
            Field::TrackNumber => "tracknumber",
        }
    }

    fn key(self) -> ItemKey {
        match self {
            Field::AlbumArtist => ItemKey::AlbumArtist,
            Field::Album => ItemKey::AlbumTitle,
            Field::Artist => ItemKey::TrackArtist,
            Field::Date => ItemKey::RecordingDate,
            Field::Genre => ItemKey::Genre,
            Field::Title => ItemKey::TrackTitle,
            Field::TrackNumber => ItemKey::TrackNumber,
        }
    }
}

/// A track's tags: the generic part, edited here, and the format-specific rest, kept as read.
pub struct Tags {
    rest: Rest,
    tag: Tag,
}

enum Rest {
    Vorbis(<VorbisComments as SplitTag>::Remainder),
    Id3v2(<Id3v2Tag as SplitTag>::Remainder),
}

pub struct Normalized {
    pub fixed_keys: Vec<String>,
    pub unprintable_keys: Vec<String>,
}

impl Tags {
    pub fn read(path: &Path) -> Result<Self> {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default();
        let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let options = ParseOptions::default();
        let (rest, tag) = match extension {
            FLAC_EXT => {
                let flac = FlacFile::read_from(&mut file, options)?;
                let (rest, tag) = flac
                    .vorbis_comments()
                    .cloned()
                    .unwrap_or_default()
                    .split_tag();
                (Rest::Vorbis(rest), tag)
            }
            OPUS_EXT => {
                let opus = OpusFile::read_from(&mut file, options)?;
                let (rest, tag) = opus.vorbis_comments().clone().split_tag();
                (Rest::Vorbis(rest), tag)
            }
            MP3_EXT => {
                let mpeg = MpegFile::read_from(&mut file, options)?;
                let (rest, tag) = mpeg.id3v2().cloned().unwrap_or_default().split_tag();
                (Rest::Id3v2(rest), tag)
            }
            _ => bail!("{} is not a supported audio file", path.display()),
        };
        Ok(Self { rest, tag })
    }

    pub fn save(self, path: &Path) -> Result<()> {
        let options = WriteOptions::default();
        match self.rest {
            Rest::Vorbis(rest) => rest.merge_tag(self.tag).save_to_path(path, options)?,
            Rest::Id3v2(rest) => rest.merge_tag(self.tag).save_to_path(path, options)?,
        }
        Ok(())
    }

    pub fn get(&self, field: Field) -> impl Iterator<Item = &str> {
        self.tag.get_strings(field.key())
    }

    pub fn first(&self, field: Field) -> Option<&str> {
        self.get(field).next()
    }

    pub fn has(&self, field: Field) -> bool {
        self.first(field).is_some()
    }

    pub fn set(&mut self, field: Field, values: Vec<String>) {
        set_key(&mut self.tag, field.key(), values);
    }

    /// The track total lives in its own key, so a track number never carries a `/`-total.
    pub fn remove_track_total(&mut self) -> bool {
        if self.tag.get(ItemKey::TrackTotal).is_none() {
            return false;
        }
        self.tag.remove_key(ItemKey::TrackTotal);
        true
    }

    /// Collapse whitespace and NFC-normalize every text value, dropping duplicates.
    pub fn normalize(&mut self) -> Normalized {
        let tag_type = self.tag.tag_type();
        let mut keys: Vec<ItemKey> = Vec::new();
        for key in self.tag.items().map(TagItem::key) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        let mut normalized = Normalized {
            fixed_keys: Vec::new(),
            unprintable_keys: Vec::new(),
        };
        for key in keys {
            let name = key
                .map_key(tag_type)
                .map_or_else(|| format!("{key:?}"), str::to_owned);
            let mut values: Vec<String> = self.tag.get_strings(key).map(str::to_owned).collect();
            if values.is_empty() {
                continue;
            }
            values.sort();
            let mut fixed: Vec<String> = values
                .iter()
                .map(|value| collapse_whitespace(value).nfc().collect())
                .collect();
            fixed.sort();
            fixed.dedup();
            if values
                .iter()
                .chain(&fixed)
                .any(|value| !value.chars().all(is_printable))
            {
                normalized.unprintable_keys.push(name.clone());
            }
            if fixed != values {
                set_key(&mut self.tag, key, fixed);
                normalized.fixed_keys.push(name);
            }
        }
        normalized
    }
}

fn set_key(tag: &mut Tag, key: ItemKey, values: Vec<String>) {
    tag.remove_key(key);
    for value in values {
        tag.push(TagItem::new(key, ItemValue::Text(value)));
    }
}
