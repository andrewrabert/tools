use std::collections::BTreeMap;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;

use serde::Serialize;

use crate::tools::file_hash_recorder::hash::Hashes;

const NANOSECOND: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Stamp {
    pub mtime: f64,
    pub size: u64,
}

impl From<&Metadata> for Stamp {
    fn from(metadata: &Metadata) -> Self {
        Stamp {
            mtime: metadata.mtime() as f64 + NANOSECOND * metadata.mtime_nsec() as f64,
            size: metadata.size(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContentKey {
    size: u64,
    hashes: Hashes,
}

#[derive(Clone, Debug, Serialize)]
pub struct Fingerprint {
    #[serde(flatten)]
    pub stamp: Stamp,
    #[serde(flatten)]
    pub hashes: Hashes,
}

impl Fingerprint {
    pub fn content_key(&self) -> ContentKey {
        ContentKey {
            size: self.stamp.size,
            hashes: self.hashes.clone(),
        }
    }
}

pub type ArchiveContents = BTreeMap<String, Fingerprint>;

#[derive(Clone, Debug, Serialize)]
pub struct FileRecord {
    #[serde(flatten)]
    pub fingerprint: Fingerprint,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub archive_contents: ArchiveContents,
}
