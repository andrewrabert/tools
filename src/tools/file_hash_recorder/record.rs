use std::collections::BTreeMap;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;

use serde_json::{Map, Value, json};

use crate::tools::file_hash_recorder::hash::Hashes;

const NANOSECOND: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Debug)]
pub struct Fingerprint {
    pub stamp: Stamp,
    pub hashes: Hashes,
}

impl Fingerprint {
    pub fn content_key(&self) -> ContentKey {
        ContentKey {
            size: self.stamp.size,
            hashes: self.hashes.clone(),
        }
    }

    pub fn to_json(&self) -> Map<String, Value> {
        let mut object = Map::new();
        object.insert("mtime".into(), json!(self.stamp.mtime));
        object.insert("size".into(), json!(self.stamp.size));
        for (name, digest) in self.hashes.named() {
            object.insert(name.into(), json!(digest));
        }
        object
    }
}

pub type ArchiveContents = BTreeMap<String, Fingerprint>;

#[derive(Clone, Debug)]
pub struct FileRecord {
    pub fingerprint: Fingerprint,
    pub archive_contents: ArchiveContents,
}

impl FileRecord {
    pub fn to_json(&self) -> Value {
        let mut object = self.fingerprint.to_json();
        if !self.archive_contents.is_empty() {
            let contents: Map<String, Value> = self
                .archive_contents
                .iter()
                .map(|(name, entry)| (name.clone(), Value::Object(entry.to_json())))
                .collect();
            object.insert("archive_contents".into(), Value::Object(contents));
        }
        Value::Object(object)
    }
}
