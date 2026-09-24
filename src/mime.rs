//! MIME type detection from file content.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use infer::Infer;

pub const TEXT: &str = "text/plain";
pub const BINARY: &str = "application/octet-stream";

/// Bytes read from the head of a file; what `infer` looks at.
const HEAD: u64 = 8192;

/// The built-in matchers plus those for types `infer` lacks, which it tries first.
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

/// A bencoded dictionary whose first key is one a torrent file starts with.
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

/// An ISO base media file whose major brand is Canon's `crx `.
fn is_cr3(head: &[u8]) -> bool {
    head.get(4..12) == Some(b"ftypcrx ")
}

/// A sync byte at the start of each of the first three 188-byte packets.
fn is_mpeg_ts(head: &[u8]) -> bool {
    const PACKET: usize = 188;
    const SYNC: u8 = 0x47;
    head.len() > 2 * PACKET && [0, PACKET, 2 * PACKET].iter().all(|&at| head[at] == SYNC)
}

/// The lowercased MIME type of the file's content: a type `infer` recognizes by
/// signature, else `text/plain` for UTF-8 text without NUL bytes, else
/// `application/octet-stream`. An empty file is plain text.
pub fn detect(path: &Path) -> Result<String> {
    let mut head = Vec::new();
    File::open(path)
        .and_then(|file| file.take(HEAD).read_to_end(&mut head))
        .with_context(|| format!("reading {}", path.display()))?;
    let mime = match INFER.get(&head) {
        Some(kind) => kind.mime_type(),
        None if is_text(&head) => TEXT,
        None => BINARY,
    };
    Ok(mime.to_ascii_lowercase())
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
