use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::tools::archive::extract::backend::Backend;
use crate::tools::archive::process;

const SIGNATURE_LENGTH: u64 = 16;
const ACE_SIGNATURE_OFFSET: usize = 7;
const GOG_MARKER_OFFSET: u64 = 271;
const GOG_MARKER: &[u8] = b"# with modifications for mojosetup and GOG.com installer.\n";
const VOLUME_DESCRIPTOR_OFFSET: u64 = 32769;
const VOLUME_DESCRIPTORS: [&[u8]; 6] = [b"CD001", b"BEA01", b"NSR02", b"NSR03", b"BOOT2", b"TEA01"];

fn read_at(file: &mut File, offset: u64, length: u64) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    file.seek(SeekFrom::Start(offset))?;
    file.by_ref().take(length).read_to_end(&mut data)?;
    Ok(data)
}

fn suffixes(archive: &Path) -> Vec<String> {
    let Some(name) = archive.file_name() else {
        return Vec::new();
    };
    let name = name.to_string_lossy().to_ascii_lowercase();
    if name.ends_with('.') {
        return Vec::new();
    }
    name.trim_start_matches('.')
        .split('.')
        .skip(1)
        .map(str::to_owned)
        .collect()
}

fn preferring(preferred: Backend, program: &str, fallback: Backend) -> Backend {
    if process::installed(program) {
        preferred
    } else {
        fallback
    }
}

pub fn backend(archive: &Path) -> Result<Backend> {
    let suffixes = suffixes(archive);
    let last = suffixes.last().map(String::as_str);
    let is_tarball = suffixes
        .iter()
        .rev()
        .nth(1)
        .is_some_and(|inner| inner == "tar");

    let mut file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let start = read_at(&mut file, 0, SIGNATURE_LENGTH)
        .with_context(|| format!("reading {}", archive.display()))?;
    let starts_with = |signature: &[u8]| start.starts_with(signature);

    if starts_with(b"ustar\x0000") || starts_with(b"ustar  \x00") {
        return Ok(Backend::GnuTar);
    }
    if starts_with(b"Rar!\x1a\x07\x00") || starts_with(b"Rar!\x1a\x07\x01\x00") {
        return Ok(Backend::Unrar);
    }
    if starts_with(b"7z\xbc\xaf'\x1c") {
        return Ok(Backend::SevenZip);
    }
    if starts_with(b"PK\x03\x04") || starts_with(b"PK\x05\x06") || starts_with(b"PK\x07\x08") {
        // 7z copes with multipart zips; unzip does not.
        return Ok(match Backend::SevenZip.check() {
            Ok(()) => Backend::SevenZip,
            Err(_) => Backend::Unzip,
        });
    }
    if starts_with(b"MSCF") {
        return Ok(Backend::Cabextract);
    }
    if starts_with(b"ISc(") {
        return Ok(Backend::Unshield);
    }
    if starts_with(b"\x04\"M\x18") {
        return Ok(Backend::Lz4);
    }
    if starts_with(b"\x1f\x8b") {
        return Ok(if last == Some("tgz") || is_tarball {
            Backend::GnuTar
        } else {
            preferring(Backend::Pigz, "pigz", Backend::Gzip)
        });
    }
    if starts_with(b"BZh") {
        return Ok(if is_tarball {
            Backend::GnuTar
        } else {
            preferring(Backend::Pbzip2, "pbzip2", Backend::Bzip2)
        });
    }
    if starts_with(b"\xfd7zXZ\x00") {
        return Ok(if last == Some("txz") || is_tarball {
            Backend::GnuTar
        } else {
            preferring(Backend::Pixz, "pixz", Backend::Xz)
        });
    }
    if starts_with(b"(\xb5/\xfd") {
        return Ok(if is_tarball {
            Backend::GnuTar
        } else {
            Backend::Zstd
        });
    }
    if starts_with(b"070701") || starts_with(b"070702") || starts_with(b"070707") {
        return Ok(Backend::Cpio);
    }
    if starts_with(b"MZ") {
        for installer in [Backend::Innoextract, Backend::Unzip] {
            installer.check()?;
            if installer.contents(archive).is_ok() {
                return Ok(installer);
            }
        }
    }
    if starts_with(b"hsqs") {
        return Ok(Backend::Unsquashfs);
    }
    if starts_with(b"!<arch>\n") {
        return Ok(Backend::SevenZip);
    }
    if starts_with(b"StuffIt (c)1997-") {
        return Ok(Backend::Unarchiver);
    }
    if starts_with(b"#!/bin/sh\n")
        && read_at(&mut file, GOG_MARKER_OFFSET, GOG_MARKER.len() as u64)? == GOG_MARKER
    {
        return Ok(Backend::GogLinux);
    }
    if start
        .get(ACE_SIGNATURE_OFFSET..)
        .is_some_and(|rest| rest.starts_with(b"**ACE**"))
    {
        return Ok(Backend::Unace);
    }

    let descriptor = read_at(&mut file, VOLUME_DESCRIPTOR_OFFSET, 5)?;
    if VOLUME_DESCRIPTORS.contains(&descriptor.as_slice())
        && Backend::Xorriso.contents(archive).is_ok()
    {
        return Ok(Backend::Xorriso);
    }

    match last {
        Some("cue") if Backend::CueSheet.contents(archive).is_ok() => {
            return Ok(Backend::CueSheet);
        }
        Some("rvz") => return Ok(Backend::DolphinRvz),
        Some("gcm") => return Ok(Backend::DolphinGcm),
        Some("nsz") => return Ok(Backend::Nsz),
        // brotli has no file signature
        Some("br") => return Ok(Backend::Brotli),
        Some("img") => return Ok(Backend::SevenZip),
        // images that are not ISO 9660, such as Blu-rays
        Some("iso") => return Ok(Backend::Udisksctl),
        // some tars lack the signature
        Some("tar") => return Ok(Backend::GnuTar),
        _ => {}
    }

    let listed_by_seven_zip = Backend::SevenZip
        .contents(archive)
        .is_ok_and(|contents| !contents.is_empty());
    if listed_by_seven_zip {
        return Ok(Backend::SevenZip);
    }
    bail!("{} is not a supported archive", archive.display());
}
