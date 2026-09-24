//! AES-CTR keystreams over buffers and file regions.
use aes::Aes128;
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
