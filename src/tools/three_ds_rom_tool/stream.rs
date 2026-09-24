//! AES-CTR keystreams over buffers and file regions.
use std::io::{self, Read, Seek, SeekFrom, Write};

use aes::Aes128;
use anyhow::{Result, bail};
use ctr::Ctr128BE;
use ctr::cipher::{KeyIvInit, StreamCipher};

use crate::tools::three_ds_rom_tool::keys::NormalKey;

pub const MIB: u64 = 1024 * 1024;
const BLOCK_LEN: u64 = 0x10;

#[derive(Clone, Copy)]
pub struct Counter(u128);

impl Counter {
    pub fn new(value: u128) -> Self {
        Counter(value)
    }

    pub fn advance(self, bytes: u64) -> Self {
        Counter(self.0.wrapping_add(u128::from(bytes / BLOCK_LEN)))
    }

    pub fn apply(self, key: NormalKey, data: &mut [u8]) {
        self.cipher(key).apply_keystream(data);
    }

    fn cipher(self, key: NormalKey) -> Ctr128BE<Aes128> {
        Ctr128BE::<Aes128>::new(&key.to_bytes().into(), &self.0.to_be_bytes().into())
    }
}

#[derive(Clone, Copy)]
pub struct Region {
    pub offset: u64,
    pub len: u64,
}

pub fn apply_region<F: Read + Write + Seek>(
    file: &mut F,
    region: Region,
    keys: &[NormalKey],
    counter: Counter,
    chunk_len: u64,
    label: &str,
) -> Result<()> {
    let mut ciphers: Vec<_> = keys.iter().map(|&key| counter.cipher(key)).collect();
    let total_mib = region.len / MIB + 1;
    let mut buffer = vec![0; usize::try_from(chunk_len.min(region.len))?];
    let mut stdout = io::stdout();
    let mut done = 0;
    while done < region.len {
        let want = usize::try_from(chunk_len.min(region.len - done))?;
        file.seek(SeekFrom::Start(region.offset + done))?;
        let read = file.read(&mut buffer[..want])?;
        if read == 0 {
            bail!("{label}: file is truncated");
        }
        let data = &mut buffer[..read];
        for cipher in &mut ciphers {
            cipher.apply_keystream(data);
        }
        file.seek(SeekFrom::Start(region.offset + done))?;
        file.write_all(data)?;
        done += u64::try_from(read)?;
        print!("\r{label}: {:4} / {total_mib:4} mb", done / MIB);
        stdout.flush()?;
    }
    println!("\r{label}: {total_mib:4} / {total_mib:4} mb... Done");
    Ok(())
}
