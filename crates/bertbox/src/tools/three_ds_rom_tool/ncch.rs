//! NCCH partition headers and their crypto flags.
use std::fmt;
use std::io::{Read, Seek, SeekFrom};

use anyhow::{Result, bail};

use crate::tools::three_ds_rom_tool::keys::{KeySet, KeyY, Keyslot, NormalKey};
use crate::tools::three_ds_rom_tool::le;
use crate::tools::three_ds_rom_tool::media::Extent;
use crate::tools::three_ds_rom_tool::stream::Counter;

pub const HEADER_LEN: usize = 0x200;
pub const EXHEADER_OFFSET: u64 = 0x200;
pub const FLAGS_OFFSET: u64 = FLAGS_AT as u64;
const FLAGS_AT: usize = 0x188;
const MAGIC: &[u8] = b"NCCH";
const MAGIC_AT: usize = 0x100;
const EXHEADER_HASH_AT: usize = 0x160;
const EXHEADER_HASH_LEN: usize = 0x20;
const EXHEADER_LEN_AT: usize = 0x180;
const EXEFS_AT: usize = 0x1A0;
const ROMFS_AT: usize = 0x1B0;
const TITLE_ID_AT: usize = 0x108;
const FLAGS_LEN: usize = 8;
const METHOD: usize = 3;
const BITS: usize = 7;

const FIXED_KEY: u8 = 0x01;
const NO_CRYPTO: u8 = 0x04;
const NEW_KEYY: u8 = 0x20;

#[derive(Clone, Copy)]
pub enum Section {
    Plain = 1,
    Exefs = 2,
    Romfs = 3,
}

pub struct Ncch {
    header: [u8; HEADER_LEN],
    pub keyy: KeyY,
    title_id: u64,
    pub flags: Flags,
    pub exheader_len: u32,
    pub exefs: Extent,
    pub romfs: Extent,
}

impl Ncch {
    pub fn read<R: Read + Seek>(file: &mut R, base: u64) -> Result<Option<Self>> {
        let mut header = [0; HEADER_LEN];
        file.seek(SeekFrom::Start(base))?;
        file.read_exact(&mut header)?;
        if &header[MAGIC_AT..MAGIC_AT + MAGIC.len()] != MAGIC {
            return Ok(None);
        }
        let mut keyy = [0; 16];
        keyy.copy_from_slice(&header[..16]);
        Ok(Some(Ncch {
            keyy: KeyY::from(keyy),
            title_id: le::u64_at(&header, TITLE_ID_AT),
            flags: Flags::parse(&header[FLAGS_AT..]),
            exheader_len: le::u32_at(&header, EXHEADER_LEN_AT),
            exefs: Extent::parse(&header, EXEFS_AT),
            romfs: Extent::parse(&header, ROMFS_AT),
            header,
        }))
    }

    pub fn counter(&self, section: Section) -> Counter {
        Counter::new((u128::from(self.title_id) << 64) | ((section as u128) << 56))
    }

    pub fn exheader_hash(&self) -> &[u8] {
        &self.header[EXHEADER_HASH_AT..EXHEADER_HASH_AT + EXHEADER_HASH_LEN]
    }

    pub fn patched_header(&self, exheader_hash: &[u8], mark_plain: bool) -> [u8; HEADER_LEN] {
        let mut header = self.header;
        header[EXHEADER_HASH_AT..EXHEADER_HASH_AT + EXHEADER_HASH_LEN]
            .copy_from_slice(exheader_hash);
        if mark_plain {
            header[FLAGS_AT + BITS] |= NO_CRYPTO;
        }
        header
    }
}

#[derive(Clone, Copy)]
pub struct Flags([u8; FLAGS_LEN]);

impl Flags {
    pub fn parse(bytes: &[u8]) -> Self {
        let mut flags = [0; FLAGS_LEN];
        flags.copy_from_slice(&bytes[..FLAGS_LEN]);
        Flags(flags)
    }

    pub fn is_plain(self) -> bool {
        self.0[BITS] & NO_CRYPTO != 0
    }

    pub fn fixed_key(self) -> bool {
        self.0[BITS] & FIXED_KEY != 0
    }

    pub fn crypto(self) -> Result<Crypto> {
        if self.fixed_key() {
            return Ok(Crypto::FixedKey);
        }
        let slot = match self.0[METHOD] {
            0x00 => Keyslot::Slot2C,
            0x01 => Keyslot::Slot25,
            0x0A => Keyslot::Slot18,
            0x0B => Keyslot::Slot1B,
            method => bail!("unknown crypto method 0x{method:02X}"),
        };
        Ok(Crypto::Keyslot(slot))
    }

    pub fn decrypted(self) -> Self {
        let mut flags = self.0;
        flags[METHOD] = 0x00;
        flags[BITS] = (flags[BITS] & !(FIXED_KEY | NEW_KEYY)) | NO_CRYPTO;
        Flags(flags)
    }

    pub fn encrypted(self, card_info: Flags, primary: bool) -> Self {
        let mut flags = self.0;
        flags[METHOD] = if primary { card_info.0[METHOD] } else { 0x00 };
        flags[BITS] = (flags[BITS] & !(FIXED_KEY | NEW_KEYY | NO_CRYPTO))
            | (card_info.0[BITS] & (FIXED_KEY | NEW_KEYY));
        Flags(flags)
    }

    pub fn to_bytes(self) -> [u8; FLAGS_LEN] {
        self.0
    }
}

#[derive(Clone, Copy)]
pub enum Crypto {
    FixedKey,
    Keyslot(Keyslot),
}

impl Crypto {
    pub fn keys(self, keyset: KeySet, keyy: KeyY) -> Keys {
        match self {
            Crypto::FixedKey => Keys::Single(NormalKey::ZERO),
            Crypto::Keyslot(Keyslot::Slot2C) => {
                Keys::Single(keyset.normal_key(Keyslot::Slot2C, keyy))
            }
            Crypto::Keyslot(slot) => Keys::Layered {
                base: keyset.normal_key(Keyslot::Slot2C, keyy),
                content: keyset.normal_key(slot, keyy),
            },
        }
    }
}

impl fmt::Display for Crypto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Crypto::FixedKey => f.write_str("Zero Key"),
            Crypto::Keyslot(slot) => write!(f, "Key {slot}"),
        }
    }
}

#[derive(Clone, Copy)]
pub enum Keys {
    Single(NormalKey),
    Layered { base: NormalKey, content: NormalKey },
}

impl Keys {
    pub fn base(self) -> NormalKey {
        match self {
            Keys::Single(key) | Keys::Layered { base: key, .. } => key,
        }
    }

    pub fn content(self) -> NormalKey {
        match self {
            Keys::Single(key) | Keys::Layered { content: key, .. } => key,
        }
    }
}
