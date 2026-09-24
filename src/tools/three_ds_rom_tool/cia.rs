//! NCSD (CCI) to CIA conversion.
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use md5::Md5;
use sha2::{Digest, Sha256};

use crate::atomic_write;
use crate::dirs;
use crate::tools::three_ds_rom_tool::exefs;
use crate::tools::three_ds_rom_tool::keys::{KeySet, Keyslot, NormalKey};
use crate::tools::three_ds_rom_tool::media::{Extent, MediaUnit};
use crate::tools::three_ds_rom_tool::ncch::{self, Ncch, Section};
use crate::tools::three_ds_rom_tool::ncsd::Ncsd;
use crate::tools::three_ds_rom_tool::rom_reader::RomReader;

const TEMP_PREFIX: &str = ".cia_";
const MEDIA_UNIT: MediaUnit = MediaUnit::new(0x200);
const COPY_CHUNK_LEN: usize = 0x80_0000;

const CERTCHAIN_LEN: usize = 0xA00;
const CERTCHAIN_RETAIL: &[u8; CERTCHAIN_LEN] = include_bytes!("certchain-retail.bin");
const CERTCHAIN_DEV_MD5: &str = "d5c3d811a7eb87340aa9f4ab1841b6c4";
const CERTCHAIN_DEV_NAME: &str = "certchain-dev.bin";
const TICKET_TMD: &[u8; 0x518] = include_bytes!("ticket-tmd.bin");

const CIA_HEADER_LEN: u32 = 0x2020;
const CIA_HEADER_PADDED_LEN: usize = 0x2040;
const TICKET_LEN: u32 = 0x350;
const META_LEN: u32 = 0x3AC0;
const TMD_BASE_LEN: u32 = 0xB34;
const TMD_SIGNATURE_PADDING_LEN: usize = 0x96C;
const TMD_PADDING_BASE_LEN: usize = 12;
const TMD_PADDING_PER_EXTRA_LEN: usize = 16;
const TICKET_TITLE_ID_AT: usize = 0x2C1C;
const TMD_TITLE_ID_AT: usize = 0x2F4C;
const TMD_SAVE_SIZE_AT: usize = 0x2F5A;
const TMD_CONTENT_COUNT_AT: usize = 0x2F9F;
const TMD_INFO_RECORDS_HASH_AT: usize = 0x2FA4;
const TMD_INFO_RECORD_AT: usize = 0x2FC7;
const TMD_INFO_TRAILING_LEN: usize = 0x8DC;
const CHUNK_RECORD_LEN: usize = 0x30;
const CHUNK_RECORD_HASH_AT: usize = 0x10;
const HASH_LEN: usize = 0x20;

const EXHEADER_LEN: usize = 0x400;
const EXHEADER_FLAGS_AT: usize = 0xD;
const EXHEADER_SD_APPLICATION: u8 = 0x02;
const EXHEADER_DEPENDENCIES: std::ops::Range<usize> = 0x40..0x1C0;
const EXHEADER_SAVE_SIZE: std::ops::Range<usize> = 0x1C0..0x1C4;
const EXEFS_TABLE_LEN: usize = 0x40;
const ICON_LEN: usize = 0x36C0;
const META_PADDING_LEN: usize = 0x180;
const META_CORE_VERSION: u32 = 2;
const META_RESERVED_LEN: usize = 0xFC;

pub struct Certchain([u8; CERTCHAIN_LEN]);

impl Certchain {
    pub fn retail() -> Self {
        Certchain(*CERTCHAIN_RETAIL)
    }

    pub fn dev(path: Option<PathBuf>) -> Result<Self> {
        let candidates = match path {
            Some(path) => vec![path],
            None => vec![
                PathBuf::from(CERTCHAIN_DEV_NAME),
                dirs::home()?.join(".3ds").join(CERTCHAIN_DEV_NAME),
            ],
        };
        candidates
            .iter()
            .find_map(|candidate| Self::read_dev(candidate))
            .context("invalid or missing dev certchain")
    }

    fn read_dev(path: &Path) -> Option<Self> {
        let mut bytes = Vec::with_capacity(CERTCHAIN_LEN);
        File::open(path)
            .ok()?
            .take(CERTCHAIN_LEN as u64)
            .read_to_end(&mut bytes)
            .ok()?;
        if hex::encode(Md5::digest(&bytes)) != CERTCHAIN_DEV_MD5 {
            return None;
        }
        bytes.try_into().ok().map(Certchain)
    }
}

pub struct Config {
    pub output: PathBuf,
    pub force: bool,
    pub ignore_bad_hashes: bool,
    pub ignore_encryption: bool,
    pub keyset: KeySet,
    pub certchain: Certchain,
}

struct Content {
    index: u16,
    extent: Extent,
    label: &'static str,
}

impl Content {
    fn offset(&self) -> u64 {
        MEDIA_UNIT.bytes(self.extent.offset)
    }

    fn size(&self) -> u64 {
        MEDIA_UNIT.bytes(self.extent.len)
    }

    fn chunk_record(&self) -> [u8; CHUNK_RECORD_LEN] {
        let mut record = [0; CHUNK_RECORD_LEN];
        record[0..4].copy_from_slice(&u32::from(self.index).to_be_bytes());
        record[4..6].copy_from_slice(&self.index.to_be_bytes());
        record[8..16].copy_from_slice(&self.size().to_be_bytes());
        record
    }
}

pub fn convert(rom: &mut RomReader, config: &Config) -> Result<()> {
    let path = rom.name().to_path_buf();
    let stem = path
        .file_stem()
        .with_context(|| format!("{}: no file name", path.display()))?;
    let mut cia_name = stem.to_os_string();
    cia_name.push(".cia");
    let cia_path = config.output.join(cia_name);
    if !config.force && cia_path.is_file() {
        bail!(
            "\"{}\" already exists. Use --force to overwrite it.",
            cia_path.display()
        );
    }

    rom.keep();
    let ncsd = Ncsd::read(rom)?;
    let not_cci = || {
        format!(
            "\"{}\" is not a CCI file (missing NCCH magic)",
            path.display()
        )
    };
    let game = Content {
        index: 0,
        extent: ncsd.partition(0).with_context(not_cci)?,
        label: "Game Executable CXI",
    };
    let game_offset = game.offset();
    let ncch = Ncch::read(rom, game_offset)?.with_context(not_cci)?;
    let mut contents = vec![game];
    for (index, label) in [(1, "Manual CFA"), (2, "Download Play child container CFA")] {
        if let Some(extent) = ncsd.partition(usize::from(index)) {
            contents.push(Content {
                index,
                extent,
                label,
            });
        }
    }

    let crypt_key = (!ncch.flags.is_plain() && !config.ignore_encryption).then(|| {
        if ncch.flags.fixed_key() {
            NormalKey::ZERO
        } else {
            config.keyset.normal_key(Keyslot::Slot2C, ncch.keyy)
        }
    });
    let state = if config.ignore_encryption {
        "ignore encryption"
    } else if ncch.flags.fixed_key() {
        "zerokey encrypted"
    } else if crypt_key.is_some() {
        "encrypted"
    } else {
        "decrypted"
    };
    println!("Converting {} ({state})...", stem.to_string_lossy());

    let plain_counter = ncch.counter(Section::Plain);
    let mut exheader = [0; EXHEADER_LEN];
    rom.seek(SeekFrom::Start(game_offset + ncch::EXHEADER_OFFSET))?;
    rom.read_exact(&mut exheader)?;
    if let Some(key) = crypt_key {
        plain_counter.apply(key, &mut exheader);
    }
    if Sha256::digest(exheader).as_slice() != ncch.exheader_hash() {
        let message = "This file may be corrupt (invalid ExtHeader hash). \
            If you are certain that the rom is decrypted, use --ignore-encryption";
        if !config.ignore_bad_hashes {
            bail!(message);
        }
        println!("{message}");
        println!("Converting anyway because --ignore-bad-hashes was passed.");
    }
    exheader[EXHEADER_FLAGS_AT] |= EXHEADER_SD_APPLICATION;
    let exheader_hash = Sha256::digest(exheader);
    let dependencies = exheader[EXHEADER_DEPENDENCIES].to_vec();
    let save_size = exheader[EXHEADER_SAVE_SIZE].to_vec();
    if let Some(key) = crypt_key {
        plain_counter.apply(key, &mut exheader);
    }
    let ncch_header = ncch.patched_header(&exheader_hash, config.ignore_encryption);

    let exefs_offset = game_offset + MEDIA_UNIT.bytes(ncch.exefs.offset);
    let exefs_counter = ncch.counter(Section::Exefs);
    let mut exefs_table = [0; EXEFS_TABLE_LEN];
    rom.seek(SeekFrom::Start(exefs_offset))?;
    rom.read_exact(&mut exefs_table)?;
    if let Some(key) = crypt_key {
        exefs_counter.apply(key, &mut exefs_table);
    }
    let icon = exefs::find(&exefs_table, b"icon").context("icon not found in the ExeFS")?;
    let icon_skip = exefs::HEADER_LEN + icon.offset;
    let mut icon = vec![0; ICON_LEN];
    rom.seek(SeekFrom::Start(exefs_offset + icon_skip))?;
    rom.read_exact(&mut icon)?;
    if let Some(key) = crypt_key {
        exefs_counter.advance(icon_skip).apply(key, &mut icon);
    }

    rom.stop_keeping();

    let (mut head, records_at) = head(&contents, &config.certchain, ncsd.media_id, &save_size)?;

    fs::create_dir_all(&config.output)
        .with_context(|| format!("creating {}", config.output.display()))?;
    let temp = atomic_write::sibling(&config.output, TEMP_PREFIX, &cia_path).create()?;
    let mut cia = File::create(&temp).with_context(|| format!("creating {}", temp.display()))?;
    cia.write_all(&head)?;

    for (number, content) in contents.iter().enumerate() {
        let mut hasher = Sha256::new();
        let (offset, len) = if number == 0 {
            let prefix = [ncch_header.as_slice(), &exheader].concat();
            hasher.update(&prefix);
            cia.write_all(&prefix)?;
            let prefix_len = u64::try_from(prefix.len())?;
            let len = content
                .size()
                .checked_sub(prefix_len)
                .context("game partition is too small")?;
            (content.offset() + prefix_len, len)
        } else {
            (content.offset(), content.size())
        };
        copy_hashed(rom, &mut cia, &mut hasher, offset, len, content.label)?;
        let hash_at = records_at + number * CHUNK_RECORD_LEN + CHUNK_RECORD_HASH_AT;
        head[hash_at..hash_at + HASH_LEN].copy_from_slice(&hasher.finalize());
    }

    let content_count = u8::try_from(contents.len())?;
    let records = &head[records_at..records_at + contents.len() * CHUNK_RECORD_LEN];
    let records_hash = Sha256::digest(records);
    head[TMD_INFO_RECORD_AT] = content_count;
    head[TMD_INFO_RECORD_AT + 1..TMD_INFO_RECORD_AT + 1 + HASH_LEN].copy_from_slice(&records_hash);
    let mut info_records = vec![0, 0, 0, content_count];
    info_records.extend_from_slice(&records_hash);
    info_records.resize(info_records.len() + TMD_INFO_TRAILING_LEN, 0);
    head[TMD_INFO_RECORDS_HASH_AT..TMD_INFO_RECORDS_HASH_AT + HASH_LEN]
        .copy_from_slice(&Sha256::digest(&info_records));

    cia.write_all(&dependencies)?;
    cia.write_all(&[0; META_PADDING_LEN])?;
    cia.write_all(&META_CORE_VERSION.to_le_bytes())?;
    cia.write_all(&[0; META_RESERVED_LEN])?;
    cia.write_all(&icon)?;

    cia.seek(SeekFrom::Start(0))?;
    cia.write_all(&head)?;

    drop(cia);
    atomic_write::replace(temp, &cia_path)?;
    println!("Wrote {}", cia_path.display());
    Ok(())
}

fn head(
    contents: &[Content],
    certchain: &Certchain,
    media_id: u64,
    save_size: &[u8],
) -> Result<(Vec<u8>, usize)> {
    let content_count = u8::try_from(contents.len())?;
    let extra_count = contents.len() - 1;
    let tmd_len = TMD_BASE_LEN + u32::try_from(extra_count * CHUNK_RECORD_LEN)?;
    let content_size: u64 = contents.iter().map(Content::size).sum();
    let content_index = contents
        .iter()
        .fold(0u8, |bits, content| bits | (0x80 >> content.index));

    let mut head = Vec::new();
    head.extend_from_slice(&CIA_HEADER_LEN.to_le_bytes());
    head.extend_from_slice(&[0; 4]);
    head.extend_from_slice(&u32::try_from(CERTCHAIN_LEN)?.to_le_bytes());
    head.extend_from_slice(&TICKET_LEN.to_le_bytes());
    head.extend_from_slice(&tmd_len.to_le_bytes());
    head.extend_from_slice(&META_LEN.to_le_bytes());
    head.extend_from_slice(&content_size.to_le_bytes());
    head.push(content_index);
    head.resize(CIA_HEADER_PADDED_LEN, 0);
    head.extend_from_slice(&certchain.0);
    head.extend_from_slice(TICKET_TMD);
    head.resize(head.len() + TMD_SIGNATURE_PADDING_LEN, 0);
    let records_at = head.len();
    for content in contents {
        head.extend_from_slice(&content.chunk_record());
    }
    head.resize(
        head.len() + TMD_PADDING_BASE_LEN + extra_count * TMD_PADDING_PER_EXTRA_LEN,
        0,
    );

    let title_id = media_id.to_be_bytes();
    head[TMD_CONTENT_COUNT_AT] = content_count;
    head[TICKET_TITLE_ID_AT..TICKET_TITLE_ID_AT + title_id.len()].copy_from_slice(&title_id);
    head[TMD_TITLE_ID_AT..TMD_TITLE_ID_AT + title_id.len()].copy_from_slice(&title_id);
    head[TMD_SAVE_SIZE_AT..TMD_SAVE_SIZE_AT + save_size.len()].copy_from_slice(save_size);
    Ok((head, records_at))
}

fn copy_hashed(
    rom: &mut RomReader,
    cia: &mut File,
    hasher: &mut Sha256,
    offset: u64,
    len: u64,
    label: &str,
) -> Result<()> {
    println!("Writing {label}...");
    rom.seek(SeekFrom::Start(offset))?;
    let mut buffer = vec![0; COPY_CHUNK_LEN];
    let mut stdout = io::stdout();
    let mut done = 0;
    while done < len {
        let want =
            usize::try_from(len - done).map_or(COPY_CHUNK_LEN, |left| left.min(COPY_CHUNK_LEN));
        let read = rom.read(&mut buffer[..want])?;
        if read == 0 {
            bail!("{label}: file is truncated");
        }
        hasher.update(&buffer[..read]);
        cia.write_all(&buffer[..read])?;
        done += u64::try_from(read)?;
        print!(
            "\r  {:>5.1}% {done:>10} / {len}",
            done as f64 / len as f64 * 100.0
        );
        stdout.flush()?;
    }
    println!();
    Ok(())
}
