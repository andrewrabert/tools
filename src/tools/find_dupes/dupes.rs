use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::tools::find_dupes::rank::RetainRank;

const FIRST_CHUNK_SIZE: u64 = 8192;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sha256Hex(String);

impl Sha256Hex {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct DuplicateSets {
    pub unique: BTreeSet<PathBuf>,
    pub duplicates: BTreeMap<Sha256Hex, Vec<PathBuf>>,
}

pub fn find(files: BTreeSet<PathBuf>) -> Result<DuplicateSets> {
    let mut unique = BTreeSet::new();

    let mut by_size: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();
    for path in files {
        let size = fs::metadata(&path)
            .with_context(|| format!("stat {}", path.display()))?
            .len();
        by_size.entry(size).or_default().push(path);
    }

    let mut by_chunk: HashMap<(u64, Vec<u8>), Vec<PathBuf>> = HashMap::new();
    for (size, paths) in by_size {
        if paths.len() == 1 {
            unique.extend(paths);
            continue;
        }
        for path in paths {
            let chunk = read_first_chunk(&path)?;
            by_chunk.entry((size, chunk)).or_default().push(path);
        }
    }

    let mut by_hash: BTreeMap<Sha256Hex, Vec<PathBuf>> = BTreeMap::new();
    for paths in by_chunk.into_values() {
        if paths.len() == 1 {
            unique.extend(paths);
            continue;
        }
        for path in paths {
            let hash = sha256(&path)?;
            by_hash.entry(hash).or_default().push(path);
        }
    }

    let mut duplicates = BTreeMap::new();
    for (hash, mut paths) in by_hash {
        if paths.len() == 1 {
            unique.extend(paths);
            continue;
        }
        paths.sort_by_cached_key(|p| RetainRank::of(p));
        duplicates.insert(hash, paths);
    }

    Ok(DuplicateSets { unique, duplicates })
}

fn read_first_chunk(path: &Path) -> Result<Vec<u8>> {
    let mut chunk = Vec::new();
    File::open(path)
        .and_then(|f| f.take(FIRST_CHUNK_SIZE).read_to_end(&mut chunk))
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(chunk)
}

fn sha256(path: &Path) -> Result<Sha256Hex> {
    let mut hasher = Sha256::new();
    File::open(path)
        .and_then(|mut f| io::copy(&mut f, &mut hasher))
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(Sha256Hex(format!("{:x}", hasher.finalize())))
}
