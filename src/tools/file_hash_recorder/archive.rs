use std::io;
use std::path::Path;

use anyhow::{Context, Result, anyhow};

use crate::tools::archive::{extract_member, extract_to, list};
use crate::tools::file_hash_recorder::hash::Hashes;
use crate::tools::file_hash_recorder::record::{ArchiveContents, Fingerprint, Stamp};
use crate::tools::file_hash_recorder::scan;

const SUFFIXES: [&str; 11] = [
    ".7z", ".bz2", ".cbz", ".gz", ".nsz", ".rar", ".rvz", ".tgz", ".xz", ".zip", ".zst",
];

#[derive(Clone, Copy)]
pub enum Extraction {
    TempDir,
    Stream,
}

pub fn has_archive_suffix(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy().to_lowercase();
        SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
    })
}

pub fn contents(archive: &Path, extraction: Extraction) -> Result<ArchiveContents> {
    match extraction {
        Extraction::TempDir => extracted(archive),
        Extraction::Stream => streamed(archive),
    }
}

fn extracted(archive: &Path) -> Result<ArchiveContents> {
    let dir = tempfile::tempdir().context("creating a temporary directory")?;
    extract_to(archive, dir.path()).with_context(|| format!("extracting {}", archive.display()))?;
    scan::tree(dir.path())
}

fn streamed(archive: &Path) -> Result<ArchiveContents> {
    let listing = list(archive)?;

    let mut contents = ArchiveContents::new();
    for (name, entry) in listing {
        let name = String::from_utf8(name).context("an archive entry name is not valid UTF-8")?;
        let stamp = Stamp {
            mtime: entry
                .mtime
                .with_context(|| format!("archive entry {name} has no mtime"))?,
            size: entry
                .size
                .with_context(|| format!("archive entry {name} has no size"))?,
        };
        let hashes = entry_hashes(archive, &name)?;
        contents.insert(name, Fingerprint { stamp, hashes });
    }
    Ok(contents)
}

fn entry_hashes(archive: &Path, name: &str) -> Result<Hashes> {
    let (reader, writer) = io::pipe().context("creating a pipe")?;
    std::thread::scope(|scope| {
        let hashing = scope.spawn(|| Hashes::of_reader(reader));
        let extracted = extract_member(archive, name, writer.into());
        let hashes = hashing
            .join()
            .map_err(|_| anyhow!("hashing thread panicked"))?;
        extracted?;
        hashes.context("reading from extract")
    })
}
