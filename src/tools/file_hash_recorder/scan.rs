use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::tools::file_hash_recorder::hash::Hashes;
use crate::tools::file_hash_recorder::record::{ArchiveContents, Fingerprint, Stamp};
use crate::tools::file_hash_recorder::walk;

pub fn fingerprint(path: &Path) -> Result<Fingerprint> {
    let metadata = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    fingerprint_as_of(path, Stamp::from(&metadata))
}

pub fn fingerprint_as_of(path: &Path, stamp: Stamp) -> Result<Fingerprint> {
    let hashes = Hashes::of_file(path).with_context(|| format!("hashing {}", path.display()))?;
    Ok(Fingerprint { stamp, hashes })
}

pub fn tree(root: &Path) -> Result<ArchiveContents> {
    let mut fingerprints = ArchiveContents::new();
    for path in walk::files_under(root)? {
        fingerprints.insert(relative_name(&path, root)?, fingerprint(&path)?);
    }
    Ok(fingerprints)
}

pub fn relative_name(path: &Path, base: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(base)
        .with_context(|| format!("{} is not under {}", path.display(), base.display()))?;
    utf8_name(relative)
}

pub fn utf8_name(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .with_context(|| format!("{} is not valid UTF-8", path.display()))
}
