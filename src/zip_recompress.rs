//! Recompression of zip archives, ported from `advzip --recompress --shrink-insane`.
//!
//! Every entry is inflated, verified against its CRC, and recompressed with zopfli
//! and libdeflate; the smallest of the original, the candidates, and storing it
//! uncompressed is kept. Extra fields and comments are dropped. The archive is
//! rewritten entry by entry in its original order.
use std::fs;
use std::io::{Read, Write};
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use flate2::read::DeflateDecoder;
use libdeflater::{CompressionLvl, Compressor};
use zopfli::{BlockType, DeflateEncoder};

use crate::atomic_write;
use crate::pool;

const TEMP_PREFIX: &str = ".zip-recompress_";

const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const END_SIGNATURE: u32 = 0x0605_4b50;
const LOCAL_FIXED: usize = 30;
const CENTRAL_FIXED: usize = 46;
const END_FIXED: usize = 22;
const MAX_COMMENT: usize = u16::MAX as usize;
const UNIQUE_DISK: u16 = 0;
const ZIP64_U16: u16 = u16::MAX;
const ZIP64_U32: u32 = u32::MAX;

const METHOD_STORE: u16 = 0;
const METHOD_DEFLATE: u16 = 8;
const VERSION_STORE: u8 = 10;
const VERSION_DEFLATE: u8 = 20;

const FLAG_ENCRYPTED: u16 = 1 << 0;
const FLAG_DEFLATE_MAXIMUM: u16 = 1 << 1;
const FLAG_DATA_DESCRIPTOR: u16 = 1 << 3;
const FLAG_EFS: u16 = 1 << 11;

const ZOPFLI_ITERATIONS: u64 = 5;
/// The size of the parts `ZopfliDeflate` compresses one at a time.
const ZOPFLI_MASTER_BLOCK_SIZE: usize = 1_000_000;
const LIBDEFLATE_LEVEL: i32 = 12;

pub struct Outcome {
    pub before: u64,
    pub after: u64,
}

/// Recompresses `path` in place, compressing entries on `workers` threads.
pub fn recompress(path: &Path, workers: NonZeroUsize) -> Result<Outcome> {
    let input = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let output = recompress_bytes(&input, workers)
        .with_context(|| format!("recompressing {}", path.display()))?;
    let temp = atomic_write::sibling(TEMP_PREFIX, path)?;
    fs::write(&temp, &output).with_context(|| format!("writing {}", temp.display()))?;
    atomic_write::replace(temp, path)?;
    Ok(Outcome {
        before: input.len() as u64,
        after: output.len() as u64,
    })
}

pub fn recompress_bytes(input: &[u8], workers: NonZeroUsize) -> Result<Vec<u8>> {
    let archive = Archive::parse(input)?;
    let mut compressed: Vec<Option<Compressed>> =
        (0..archive.entries.len()).map(|_| None).collect();
    pool::run(
        (0..archive.entries.len()).collect(),
        workers,
        |&index| shrink(&archive.entries[index]),
        |index, result| {
            let entry = &archive.entries[index];
            compressed[index] = Some(
                result.with_context(|| format!("entry {}", String::from_utf8_lossy(entry.name)))?,
            );
            Ok(())
        },
    )?;
    let compressed: Vec<Compressed> = compressed.into_iter().flatten().collect();
    Ok(write(&archive.entries, &compressed))
}

/// The fields of a central directory record that survive recompression.
struct Entry<'a> {
    version_made_by: u16,
    version_needed: u8,
    os_needed_to_extract: u8,
    flags: u16,
    method: u16,
    time: u16,
    date: u16,
    crc32: u32,
    uncompressed_size: u32,
    internal_attributes: u16,
    external_attributes: u32,
    name: &'a [u8],
    data: &'a [u8],
}

struct Archive<'a> {
    entries: Vec<Entry<'a>>,
}

struct Compressed {
    data: Vec<u8>,
    method: u16,
    version_needed: u8,
    flags: u16,
}

impl<'a> Archive<'a> {
    fn parse(input: &'a [u8]) -> Result<Self> {
        let end = find_end(input)?;
        let count = read_u16(input, end + 10) as usize;
        ensure!(
            read_u16(input, end + 8) as usize == count,
            "multi-disk archives are not supported"
        );
        ensure!(
            count != ZIP64_U16 as usize,
            "zip64 archives are not supported"
        );
        let central_size = read_u32(input, end + 12) as usize;
        let central_offset = read_u32(input, end + 16) as usize;
        ensure!(
            central_offset
                .checked_add(central_size)
                .is_some_and(|central_end| central_end <= end),
            "invalid central directory"
        );
        let mut entries = Vec::with_capacity(count);
        let mut offset = central_offset;
        for _ in 0..count {
            let (entry, next) = Entry::parse(input, offset)?;
            entries.push(entry);
            offset = next;
        }
        ensure!(
            offset == central_offset + central_size,
            "invalid central directory size"
        );
        Ok(Self { entries })
    }
}

impl<'a> Entry<'a> {
    fn parse(input: &'a [u8], offset: usize) -> Result<(Self, usize)> {
        ensure!(
            offset + CENTRAL_FIXED <= input.len() && read_u32(input, offset) == CENTRAL_SIGNATURE,
            "invalid central directory record at offset {offset}"
        );
        let version_made_by = read_u16(input, offset + 4);
        let version_needed = input[offset + 6];
        let os_needed_to_extract = input[offset + 7];
        let flags = read_u16(input, offset + 8);
        let method = read_u16(input, offset + 10);
        let time = read_u16(input, offset + 12);
        let date = read_u16(input, offset + 14);
        let crc32 = read_u32(input, offset + 16);
        let compressed_size = read_u32(input, offset + 20);
        let uncompressed_size = read_u32(input, offset + 24);
        let name_length = read_u16(input, offset + 28) as usize;
        let extra_length = read_u16(input, offset + 30) as usize;
        let comment_length = read_u16(input, offset + 32) as usize;
        let internal_attributes = read_u16(input, offset + 36);
        let external_attributes = read_u32(input, offset + 38);
        let local_offset = read_u32(input, offset + 42) as usize;
        let name_start = offset + CENTRAL_FIXED;
        let next = name_start + name_length + extra_length + comment_length;
        ensure!(next <= input.len(), "truncated central directory record");
        let name = &input[name_start..name_start + name_length];
        ensure!(
            compressed_size != ZIP64_U32
                && uncompressed_size != ZIP64_U32
                && local_offset != ZIP64_U32 as usize,
            "zip64 entries are not supported"
        );
        ensure!(
            flags & FLAG_ENCRYPTED == 0,
            "encrypted entries are not supported"
        );
        ensure!(
            local_offset + LOCAL_FIXED <= input.len()
                && read_u32(input, local_offset) == LOCAL_SIGNATURE,
            "invalid local header at offset {local_offset}"
        );
        let local_name_length = read_u16(input, local_offset + 26) as usize;
        let local_extra_length = read_u16(input, local_offset + 28) as usize;
        let data_start = local_offset + LOCAL_FIXED + local_name_length + local_extra_length;
        let data_end = data_start + compressed_size as usize;
        ensure!(data_end <= input.len(), "truncated entry data");
        let entry = Self {
            version_made_by,
            version_needed,
            os_needed_to_extract,
            flags,
            method,
            time,
            date,
            crc32,
            uncompressed_size,
            internal_attributes,
            external_attributes,
            name,
            data: &input[data_start..data_end],
        };
        Ok((entry, next))
    }

    fn inflate(&self) -> Result<Vec<u8>> {
        let size = self.uncompressed_size as usize;
        let data = match self.method {
            METHOD_STORE => self.data.to_vec(),
            METHOD_DEFLATE => {
                let mut data = Vec::with_capacity(size);
                DeflateDecoder::new(self.data)
                    .take(size as u64 + 1)
                    .read_to_end(&mut data)
                    .context("inflating")?;
                data
            }
            method => bail!("compression method {method} is not supported"),
        };
        ensure!(data.len() == size, "uncompressed size mismatch");
        ensure!(crc32fast::hash(&data) == self.crc32, "invalid crc");
        Ok(data)
    }
}

/// Picks the smallest encoding of an entry, as `zip_entry::shrink` does for a
/// standard archive at the insane level.
fn shrink(entry: &Entry) -> Result<Compressed> {
    let uncompressed = entry.inflate()?;
    let mut best = Compressed {
        data: entry.data.to_vec(),
        method: entry.method,
        version_needed: entry.version_needed,
        flags: entry.flags,
    };

    // zopfli replaces the original only when strictly smaller.
    if let Some(candidate) = zopfli(&uncompressed)
        && candidate.len() < best.data.len()
    {
        best = deflated(candidate);
    }
    // libdeflate covers the corner cases where it beats zopfli, and takes ties.
    if let Some(candidate) = libdeflate(&uncompressed)
        && candidate.len() <= best.data.len()
    {
        best = deflated(candidate);
    }
    if uncompressed.len() <= best.data.len() {
        best = Compressed {
            data: uncompressed,
            method: METHOD_STORE,
            version_needed: VERSION_STORE,
            flags: 0,
        };
    }
    // The EFS flag of the original is preserved.
    best.flags |= entry.flags & FLAG_EFS;
    Ok(best)
}

fn deflated(data: Vec<u8>) -> Compressed {
    Compressed {
        data,
        method: METHOD_DEFLATE,
        version_needed: VERSION_DEFLATE,
        flags: FLAG_DEFLATE_MAXIMUM,
    }
}

/// Zopfli at 5 iterations, fed in master blocks exactly as `ZopfliDeflate` splits
/// them so the output matches the C library byte for byte.
fn zopfli(input: &[u8]) -> Option<Vec<u8>> {
    let options = zopfli::Options {
        iteration_count: NonZeroU64::new(ZOPFLI_ITERATIONS)?,
        ..zopfli::Options::default()
    };
    let mut output = Vec::new();
    let mut encoder = DeflateEncoder::new(options, BlockType::Dynamic, &mut output);
    for part in input.chunks(ZOPFLI_MASTER_BLOCK_SIZE) {
        encoder.write_all(part).ok()?;
    }
    encoder.finish().ok()?;
    Some(output)
}

/// libdeflate at level 12, given a buffer the size of the input as advzip does;
/// output that does not fit is discarded.
fn libdeflate(input: &[u8]) -> Option<Vec<u8>> {
    let level = CompressionLvl::new(LIBDEFLATE_LEVEL).ok()?;
    let mut output = vec![0; input.len()];
    let size = Compressor::new(level)
        .deflate_compress(input, &mut output)
        .ok()?;
    output.truncate(size);
    Some(output)
}

fn write(entries: &[Entry], compressed: &[Compressed]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut local_offsets = Vec::with_capacity(entries.len());
    for (entry, compressed) in entries.iter().zip(compressed) {
        local_offsets.push(out.len() as u32);
        write_u32(&mut out, LOCAL_SIGNATURE);
        out.push(compressed.version_needed);
        out.push(entry.os_needed_to_extract);
        write_u16(&mut out, compressed.flags & !FLAG_DATA_DESCRIPTOR);
        write_u16(&mut out, compressed.method);
        write_u16(&mut out, entry.time);
        write_u16(&mut out, entry.date);
        write_u32(&mut out, entry.crc32);
        write_u32(&mut out, compressed.data.len() as u32);
        write_u32(&mut out, entry.uncompressed_size);
        write_u16(&mut out, entry.name.len() as u16);
        write_u16(&mut out, 0);
        out.extend_from_slice(entry.name);
        out.extend_from_slice(&compressed.data);
    }
    let central_offset = out.len() as u32;
    for ((entry, compressed), local_offset) in entries.iter().zip(compressed).zip(local_offsets) {
        write_u32(&mut out, CENTRAL_SIGNATURE);
        write_u16(&mut out, entry.version_made_by);
        out.push(compressed.version_needed);
        out.push(entry.os_needed_to_extract);
        write_u16(&mut out, compressed.flags & !FLAG_DATA_DESCRIPTOR);
        write_u16(&mut out, compressed.method);
        write_u16(&mut out, entry.time);
        write_u16(&mut out, entry.date);
        write_u32(&mut out, entry.crc32);
        write_u32(&mut out, compressed.data.len() as u32);
        write_u32(&mut out, entry.uncompressed_size);
        write_u16(&mut out, entry.name.len() as u16);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, UNIQUE_DISK);
        write_u16(&mut out, entry.internal_attributes);
        write_u32(&mut out, entry.external_attributes);
        write_u32(&mut out, local_offset);
        out.extend_from_slice(entry.name);
    }
    let central_size = out.len() as u32 - central_offset;
    write_u32(&mut out, END_SIGNATURE);
    write_u16(&mut out, UNIQUE_DISK);
    write_u16(&mut out, UNIQUE_DISK);
    write_u16(&mut out, entries.len() as u16);
    write_u16(&mut out, entries.len() as u16);
    write_u32(&mut out, central_size);
    write_u32(&mut out, central_offset);
    write_u16(&mut out, 0);
    out
}

/// Offset of the end of central directory record, searched backwards past any comment.
fn find_end(input: &[u8]) -> Result<usize> {
    ensure!(input.len() >= END_FIXED, "not a zip archive");
    let last = input.len() - END_FIXED;
    let first = last.saturating_sub(MAX_COMMENT);
    (first..=last)
        .rev()
        .find(|&offset| {
            read_u32(input, offset) == END_SIGNATURE
                && offset + END_FIXED + read_u16(input, offset + 20) as usize == input.len()
        })
        .context("end of central directory not found")
}

fn read_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([input[offset], input[offset + 1]])
}

fn read_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ])
}

fn write_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Write};
    use std::num::NonZeroUsize;

    use zip::write::{FullFileOptions, SimpleFileOptions};
    use zip::{CompressionMethod, ZipArchive, ZipWriter};

    use super::*;

    const TEXT: &[u8] = b"the quick brown fox jumps over the lazy dog, again and again and again";

    fn archive_with_text(repeat: usize) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer.set_comment("archive comment").unwrap();
        let mut deflated =
            FullFileOptions::default().compression_method(CompressionMethod::Deflated);
        deflated.add_extra_field(0x1234, [1, 2, 3], false).unwrap();
        writer.start_file("text.txt", deflated.clone()).unwrap();
        writer.write_all(&TEXT.repeat(repeat)).unwrap();
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file("random.bin", stored).unwrap();
        writer.write_all(&noise(4096)).unwrap();
        writer.start_file("empty", deflated).unwrap();
        writer
            .add_directory("dir/", SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap().into_inner()
    }

    /// Bytes from a 64-bit LCG, which deflate cannot shrink.
    fn noise(len: usize) -> Vec<u8> {
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        (0..len)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                (state >> 56) as u8
            })
            .collect()
    }

    fn entries(archive: &[u8]) -> Vec<(String, Vec<u8>, CompressionMethod)> {
        let mut archive = ZipArchive::new(Cursor::new(archive)).unwrap();
        (0..archive.len())
            .map(|index| {
                let mut file = archive.by_index(index).unwrap();
                let mut data = Vec::new();
                file.read_to_end(&mut data).unwrap();
                (file.name().unwrap().into_owned(), data, file.compression())
            })
            .collect()
    }

    #[test]
    fn keeps_contents_and_drops_metadata() {
        let input = archive_with_text(200);
        let output = recompress_bytes(&input, NonZeroUsize::MIN).unwrap();
        assert!(output.len() < input.len());
        let before = entries(&input);
        let after = entries(&output);
        assert_eq!(before.len(), after.len());
        for ((name, data, _), (after_name, after_data, _)) in before.iter().zip(&after) {
            assert_eq!(name, after_name);
            assert_eq!(data, after_data);
        }
        let reopened = ZipArchive::new(Cursor::new(&output)).unwrap();
        assert!(reopened.comment().is_empty());
        let text = ZipArchive::new(Cursor::new(&output))
            .unwrap()
            .by_name("text.txt")
            .unwrap()
            .extra_data()
            .map(|extra| extra.len());
        assert_eq!(text.unwrap_or(0), 0);
    }

    #[test]
    fn stores_incompressible_and_empty_entries() {
        let output = recompress_bytes(&archive_with_text(1), NonZeroUsize::MIN).unwrap();
        for (name, _, method) in entries(&output) {
            let expected = match name.as_str() {
                "text.txt" => CompressionMethod::Deflated,
                _ => CompressionMethod::Stored,
            };
            assert_eq!(method, expected, "{name}");
        }
    }

    #[test]
    fn is_idempotent_and_thread_count_independent() {
        let input = archive_with_text(500);
        let once = recompress_bytes(&input, NonZeroUsize::MIN).unwrap();
        let twice = recompress_bytes(&once, NonZeroUsize::MIN).unwrap();
        let parallel = recompress_bytes(&input, NonZeroUsize::new(4).unwrap()).unwrap();
        assert_eq!(once, twice);
        assert_eq!(once, parallel);
    }

    #[test]
    fn rejects_non_zip() {
        assert!(recompress_bytes(b"not a zip at all, definitely not", NonZeroUsize::MIN).is_err());
    }
}
