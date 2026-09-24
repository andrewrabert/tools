//! Decryption and encryption of NCSD partitions, streamed into a new file.
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::atomic_write;
use crate::tools::three_ds_rom_tool::exefs;
use crate::tools::three_ds_rom_tool::keys::KeySet;
use crate::tools::three_ds_rom_tool::media::MediaUnit;
use crate::tools::three_ds_rom_tool::ncch::{self, Keys, Ncch, Section};
use crate::tools::three_ds_rom_tool::ncsd::{self, Ncsd};
use crate::tools::three_ds_rom_tool::rom_reader::RomReader;
use crate::tools::three_ds_rom_tool::stream::{MIB, Region};

const TEMP_PREFIX: &str = ".3ds_";
const EXHEADER_LEN: u64 = 0x800;
const EXEFS_TABLE_LEN: usize = 0xA0;
const CHUNK_LEN: u64 = 16 * MIB;

#[derive(Clone, Copy)]
pub enum Direction {
    Decrypt,
    Encrypt,
}

impl Direction {
    fn verb(self) -> &'static str {
        match self {
            Direction::Decrypt => "Decrypting",
            Direction::Encrypt => "Encrypting",
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Direction::Decrypt => "decrypted",
            Direction::Encrypt => "encrypted",
        }
    }
}

struct Partition {
    index: usize,
    base: u64,
    media_unit: MediaUnit,
    ncch: Ncch,
}

pub fn run(
    rom: &mut RomReader,
    direction: Direction,
    keyset: KeySet,
    output: Option<&Path>,
    force: bool,
) -> Result<()> {
    let output = match output {
        Some(output) => output.to_path_buf(),
        None => default_output(rom.name(), direction)?,
    };
    if output.exists() {
        if fs::canonicalize(rom.path())? == fs::canonicalize(&output)? {
            bail!("\"{}\" is the input ROM.", output.display());
        }
        if !force {
            bail!(
                "\"{}\" already exists. Use --force to overwrite it.",
                output.display()
            );
        }
    }
    println!("{} -> {}", rom.path().display(), output.display());

    let parent = match output.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let temp = atomic_write::sibling(parent, TEMP_PREFIX, &output).create()?;
    let mut file = File::create(&temp).with_context(|| format!("opening {}", temp.display()))?;
    crypt_rom(rom, &mut file, direction, keyset)?;
    atomic_write::replace(temp, &output)
}

/// `game.3ds` -> `game.decrypted.3ds`; an existing `.decrypted`/`.encrypted` tag is replaced.
fn default_output(path: &Path, direction: Direction) -> Result<PathBuf> {
    let stem = path
        .file_stem()
        .with_context(|| format!("{}: no file name", path.display()))?;
    let stem = Path::new(stem);
    let base = match stem.extension() {
        Some(tag) if tag == "decrypted" || tag == "encrypted" => {
            stem.file_stem().unwrap_or(stem.as_os_str())
        }
        _ => stem.as_os_str(),
    };
    let mut name = base.to_os_string();
    name.push(".");
    name.push(direction.tag());
    if let Some(extension) = path.extension() {
        name.push(".");
        name.push(extension);
    }
    Ok(path.with_file_name(name))
}

fn crypt_rom(
    rom: &mut RomReader,
    out: &mut File,
    direction: Direction,
    keyset: KeySet,
) -> Result<()> {
    let mut header = vec![0; ncsd::HEADER_LEN];
    rom.read_exact(&mut header)
        .context("reading the NCSD header")?;
    let ncsd = Ncsd::read(&mut Cursor::new(&header))?;
    out.write_all(&header)?;
    let mut at = u64::try_from(header.len())?;

    for (index, extent) in ncsd.partitions().enumerate() {
        let Some(extent) = extent else {
            println!("Partition {index} Not found... Skipping...");
            continue;
        };
        let base = ncsd.media_unit.bytes(extent.offset);
        copy_until(rom, out, &mut at, base)?;
        let mut header = [0; ncch::HEADER_LEN];
        rom.read_exact(&mut header)
            .with_context(|| format!("reading partition {index}'s NCCH header"))?;
        at += u64::try_from(header.len())?;
        let Some(ncch) = Ncch::read(&mut Cursor::new(&header), 0)? else {
            println!("Partition {index} Unable to read NCCH header");
            out.write_all(&header)?;
            continue;
        };

        let (crypto, flags) = match (direction, ncch.flags.is_plain()) {
            (Direction::Decrypt, true) => {
                println!("Partition {index}: Already Decrypted?...");
                out.write_all(&header)?;
                continue;
            }
            (Direction::Encrypt, false) => {
                println!("Partition {index}: Already Encrypted?...");
                out.write_all(&header)?;
                continue;
            }
            (Direction::Decrypt, false) => (ncch.flags.crypto()?, ncch.flags.decrypted()),
            (Direction::Encrypt, true) => {
                let flags = ncch.flags.encrypted(ncsd.card_info_flags, index == 0);
                (flags.crypto()?, flags)
            }
        };
        if matches!(direction, Direction::Decrypt) && index == 0 {
            println!("Encryption Method: {crypto}");
        }

        let flags = flags.to_bytes();
        let flags_at = usize::try_from(ncch::FLAGS_OFFSET)?;
        header[flags_at..flags_at + flags.len()].copy_from_slice(&flags);
        out.write_all(&header)?;

        let keys = crypto.keys(keyset, ncch.keyy);
        let partition = Partition {
            index,
            base,
            media_unit: ncsd.media_unit,
            ncch,
        };
        crypt_partition(rom, out, &mut at, &partition, keys, direction)?;
    }

    io::copy(rom, out).context("copying the rest of the ROM")?;
    println!("Done...");
    Ok(())
}

fn crypt_partition(
    rom: &mut RomReader,
    out: &mut File,
    at: &mut u64,
    partition: &Partition,
    keys: Keys,
    direction: Direction,
) -> Result<()> {
    let Partition {
        index,
        base,
        media_unit,
        ref ncch,
    } = *partition;
    let verb = direction.verb();

    if ncch.exheader_len > 0 {
        let counter = ncch.counter(Section::Plain);
        crypt_region(
            rom,
            out,
            at,
            Region {
                offset: base + media_unit.bytes(1),
                len: EXHEADER_LEN,
            },
            &format!("Partition {index} ExeFS: {verb}: ExHeader"),
            |done, data| counter.advance(done).apply(keys.base(), data),
        )?;
    }

    if ncch.exefs.len > 0 {
        let exefs = base + media_unit.bytes(ncch.exefs.offset);
        let header_len = media_unit.bytes(1);
        let counter = ncch.counter(Section::Exefs);
        copy_until(rom, out, at, exefs)?;
        let mut header = vec![0; usize::try_from(header_len)?];
        rom.read_exact(&mut header)
            .with_context(|| format!("reading partition {index}'s ExeFS header"))?;
        let table = match direction {
            Direction::Decrypt => {
                counter.apply(keys.base(), &mut header);
                header[..EXEFS_TABLE_LEN].to_vec()
            }
            Direction::Encrypt => {
                let table = header[..EXEFS_TABLE_LEN].to_vec();
                counter.apply(keys.base(), &mut header);
                table
            }
        };
        out.write_all(&header)?;
        *at += header_len;
        println!("Partition {index} ExeFS: {verb}: ExeFS Filename Table... Done");

        let code = match keys {
            Keys::Layered {
                base: base_key,
                content,
            } => exefs::find(&table, b".code").map(|code| (code, base_key, content)),
            Keys::Single(_) => None,
        };
        crypt_region(
            rom,
            out,
            at,
            Region {
                offset: exefs + header_len,
                len: media_unit.bytes(ncch.exefs.len - 1),
            },
            &format!("Partition {index} ExeFS: {verb}"),
            |done, data| {
                counter.advance(header_len + done).apply(keys.base(), data);
                let Some((code, base_key, content)) = &code else {
                    return;
                };
                let from = code.offset.max(done);
                let to = (code.offset + code.len).min(done + data.len() as u64);
                if from < to {
                    let code_data = &mut data[(from - done) as usize..(to - done) as usize];
                    let code_counter = counter.advance(header_len + from);
                    code_counter.apply(*content, code_data);
                    code_counter.apply(*base_key, code_data);
                }
            },
        )?;
    } else {
        println!("Partition {index} ExeFS: No Data... Skipping...");
    }

    if ncch.romfs.offset != 0 {
        let counter = ncch.counter(Section::Romfs);
        crypt_region(
            rom,
            out,
            at,
            Region {
                offset: base + media_unit.bytes(ncch.romfs.offset),
                len: media_unit.bytes(ncch.romfs.len),
            },
            &format!("Partition {index} RomFS: {verb}"),
            |done, data| counter.advance(done).apply(keys.content(), data),
        )?;
    } else {
        println!("Partition {index} RomFS: No Data... Skipping...");
    }

    Ok(())
}

fn crypt_region(
    rom: &mut RomReader,
    out: &mut File,
    at: &mut u64,
    region: Region,
    label: &str,
    mut crypt: impl FnMut(u64, &mut [u8]),
) -> Result<()> {
    copy_until(rom, out, at, region.offset)?;
    let total_mib = region.len / MIB + 1;
    let mut buffer = vec![0; usize::try_from(CHUNK_LEN.min(region.len))?];
    let mut stdout = io::stdout();
    let mut done = 0;
    while done < region.len {
        let data = &mut buffer[..usize::try_from(CHUNK_LEN.min(region.len - done))?];
        rom.read_exact(data)
            .with_context(|| format!("{label}: file is truncated"))?;
        crypt(done, data);
        out.write_all(data)?;
        done += u64::try_from(data.len())?;
        print!("\r{label}: {:4} / {total_mib:4} mb", done / MIB);
        stdout.flush()?;
    }
    *at += region.len;
    println!("\r{label}: {total_mib:4} / {total_mib:4} mb... Done");
    Ok(())
}

fn copy_until(rom: &mut RomReader, out: &mut File, at: &mut u64, to: u64) -> Result<()> {
    let Some(len) = to.checked_sub(*at) else {
        bail!("ROM regions out of order at offset {to:#x}");
    };
    let copied = io::copy(&mut rom.by_ref().take(len), out)?;
    if copied < len {
        bail!("ROM is truncated at offset {:#x}", *at + copied);
    }
    *at = to;
    Ok(())
}
