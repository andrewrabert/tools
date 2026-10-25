//! MIME type detection from file content.

use std::cmp::Ordering;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use infer::Infer;

const TEXT: &str = "text/plain";
const BINARY: &str = "application/octet-stream";

const HEAD: u64 = 8192;

static INFER: LazyLock<Infer> = LazyLock::new(|| {
    let mut infer = Infer::new();
    infer.add("application/x-ace-compressed", "ace", is_ace);
    infer.add("application/x-bittorrent", "torrent", is_torrent);
    infer.add("image/x-canon-cr3", "cr3", is_cr3);
    infer.add("video/mp2t", "ts", is_mpeg_ts);
    infer
});

fn is_ace(head: &[u8]) -> bool {
    head.get(7..14) == Some(b"**ACE**")
}

fn is_torrent(head: &[u8]) -> bool {
    const FIRST_KEYS: [&[u8]; 6] = [
        b"d8:announce",
        b"d13:announce-list",
        b"d7:comment",
        b"d10:created by",
        b"d13:creation date",
        b"d4:info",
    ];
    FIRST_KEYS.iter().any(|key| head.starts_with(key))
}

fn is_cr3(head: &[u8]) -> bool {
    const FTYP_CANON_BRAND: &[u8] = b"ftypcrx ";
    head.get(4..4 + FTYP_CANON_BRAND.len()) == Some(FTYP_CANON_BRAND)
}

fn is_mpeg_ts(head: &[u8]) -> bool {
    const PACKET: usize = 188;
    const SYNC: u8 = 0x47;
    head.len() > 2 * PACKET && [0, PACKET, 2 * PACKET].iter().all(|&at| head[at] == SYNC)
}

#[derive(Clone)]
pub enum MimeType {
    Text,
    Binary,
    Other(String),
}

impl MimeType {
    fn named(name: &str) -> Self {
        let name = name.to_ascii_lowercase();
        match name.as_str() {
            TEXT => MimeType::Text,
            BINARY => MimeType::Binary,
            _ => MimeType::Other(name),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            MimeType::Text => TEXT,
            MimeType::Binary => BINARY,
            MimeType::Other(name) => name,
        }
    }
}

impl fmt::Display for MimeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq for MimeType {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for MimeType {}

impl Ord for MimeType {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl PartialOrd for MimeType {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn detect(path: &Path) -> Result<MimeType> {
    let mut head = Vec::new();
    File::open(path)
        .and_then(|file| file.take(HEAD).read_to_end(&mut head))
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(match INFER.get(&head) {
        Some(kind) => MimeType::named(kind.mime_type()),
        None if is_text(&head) => MimeType::Text,
        None => MimeType::Binary,
    })
}

/// Whether the head of a file reads as UTF-8 text; a multi-byte character the
/// cutoff splits does not count against it.
fn is_text(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(error) => error.error_len().is_none() && head.len() as u64 == HEAD,
    }
}
