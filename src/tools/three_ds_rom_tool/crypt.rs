//! Decryption and encryption of NCSD partitions, in place or into a copy.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

use crate::tools::three_ds_rom_tool::exefs;
use crate::tools::three_ds_rom_tool::keys::KeySet;
use crate::tools::three_ds_rom_tool::media::MediaUnit;
use crate::tools::three_ds_rom_tool::ncch::{self, Keys, Ncch, Section};
use crate::tools::three_ds_rom_tool::ncsd::Ncsd;
use crate::tools::three_ds_rom_tool::stream::{MIB, Region, apply_region};

const EXHEADER_LEN: u64 = 0x800;
const EXEFS_TABLE_LEN: usize = 0xA0;
const ROMFS_CHUNK_LEN: u64 = 16 * MIB;

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

/// Where the processed ROM is written.
pub enum Target<'a> {
    /// Modify the input ROM itself.
    InPlace,
    /// Write a new file. `output` defaults to `<stem>.<tag>.<ext>` next to the input.
    Copy {
        output: Option<&'a Path>,
        force: bool,
    },
}

struct Partition {
    index: usize,
    base: u64,
    media_unit: MediaUnit,
    ncch: Ncch,
}

pub fn run(path: &Path, direction: Direction, keyset: KeySet, target: &Target) -> Result<()> {
    match *target {
        Target::InPlace => {
            println!("{}", path.display());
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .with_context(|| format!("opening {}", path.display()))?;
            crypt_file(&mut file, direction, keyset)
        }
        Target::Copy { output, force } => {
            let output = match output {
                Some(output) => output.to_path_buf(),
                None => default_output(path, direction)?,
            };
            crypt_copy(path, &output, force, direction, keyset)
        }
    }
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

fn crypt_copy(
    input: &Path,
    output: &Path,
    force: bool,
    direction: Direction,
    keyset: KeySet,
) -> Result<()> {
    if output.exists() {
        if fs::canonicalize(input)? == fs::canonicalize(output)? {
            bail!(
                "\"{}\" is the input ROM. Use --in-place to modify it.",
                output.display()
            );
        }
        if !force {
            bail!(
                "\"{}\" already exists. Use --force to overwrite it.",
                output.display()
            );
        }
    }
    println!("{} -> {}", input.display(), output.display());

    let parent = match output.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let mut temp = NamedTempFile::new_in(parent)
        .with_context(|| format!("creating a temporary file in {}", parent.display()))?;
    let mut rom = File::open(input).with_context(|| format!("opening {}", input.display()))?;
    io::copy(&mut rom, temp.as_file_mut())
        .with_context(|| format!("copying {}", input.display()))?;
    fs::set_permissions(temp.path(), rom.metadata()?.permissions())?;
    crypt_file(temp.as_file_mut(), direction, keyset)?;

    let persisted = if force {
        temp.persist(output)
    } else {
        temp.persist_noclobber(output)
    };
    persisted.with_context(|| format!("writing {}", output.display()))?;
    Ok(())
}

fn crypt_file(file: &mut File, direction: Direction, keyset: KeySet) -> Result<()> {
    let ncsd = Ncsd::read(file)?;

    for (index, extent) in ncsd.partitions().enumerate() {
        let Some(extent) = extent else {
            println!("Partition {index} Not found... Skipping...");
            continue;
        };
        let base = ncsd.media_unit.bytes(extent.offset);
        let Some(ncch) = Ncch::read(file, base)? else {
            println!("Partition {index} Unable to read NCCH header");
            continue;
        };

        let (crypto, flags) = match (direction, ncch.flags.is_plain()) {
            (Direction::Decrypt, true) => {
                println!("Partition {index}: Already Decrypted?...");
                continue;
            }
            (Direction::Encrypt, false) => {
                println!("Partition {index}: Already Encrypted?...");
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

        let keys = crypto.keys(keyset, ncch.keyy);
        let partition = Partition {
            index,
            base,
            media_unit: ncsd.media_unit,
            ncch,
        };
        crypt_partition(file, &partition, keys, direction)?;

        file.seek(SeekFrom::Start(base + ncch::FLAGS_OFFSET))?;
        file.write_all(&flags.to_bytes())?;
    }

    println!("Done...");
    Ok(())
}

fn crypt_partition(
    file: &mut File,
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
        apply_region(
            file,
            Region {
                offset: base + media_unit.bytes(1),
                len: EXHEADER_LEN,
            },
            &[keys.base()],
            ncch.counter(Section::Plain),
            MIB,
            &format!("Partition {index} ExeFS: {verb}: ExHeader"),
        )?;
    }

    if ncch.exefs.len > 0 {
        let exefs = base + media_unit.bytes(ncch.exefs.offset);
        let header_len = media_unit.bytes(1);
        let counter = ncch.counter(Section::Exefs);
        let header = Region {
            offset: exefs,
            len: header_len,
        };
        let header_label = format!("Partition {index} ExeFS: {verb}: ExeFS Filename Table");

        if matches!(direction, Direction::Decrypt) {
            apply_region(file, header, &[keys.base()], counter, MIB, &header_label)?;
        }

        if let Keys::Layered {
            base: base_key,
            content,
        } = keys
        {
            let mut table = [0; EXEFS_TABLE_LEN];
            file.seek(SeekFrom::Start(exefs))?;
            file.read_exact(&mut table)?;
            if let Some(code) = exefs::find(&table, b".code") {
                let skip = header_len + code.offset;
                apply_region(
                    file,
                    Region {
                        offset: exefs + skip,
                        len: code.len,
                    },
                    &[content, base_key],
                    counter.advance(skip),
                    MIB,
                    &format!("Partition {index} ExeFS: {verb}: .code"),
                )?;
            }
        }

        if matches!(direction, Direction::Encrypt) {
            apply_region(file, header, &[keys.base()], counter, MIB, &header_label)?;
        }

        apply_region(
            file,
            Region {
                offset: exefs + header_len,
                len: media_unit.bytes(ncch.exefs.len - 1),
            },
            &[keys.base()],
            counter.advance(header_len),
            MIB,
            &format!("Partition {index} ExeFS: {verb}"),
        )?;
    } else {
        println!("Partition {index} ExeFS: No Data... Skipping...");
    }

    if ncch.romfs.offset != 0 {
        apply_region(
            file,
            Region {
                offset: base + media_unit.bytes(ncch.romfs.offset),
                len: media_unit.bytes(ncch.romfs.len),
            },
            &[keys.content()],
            ncch.counter(Section::Romfs),
            ROMFS_CHUNK_LEN,
            &format!("Partition {index} RomFS: {verb}"),
        )?;
    } else {
        println!("Partition {index} RomFS: No Data... Skipping...");
    }

    Ok(())
}
