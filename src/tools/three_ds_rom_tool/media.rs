//! Media-unit sizes and the extents measured in them.
use crate::tools::three_ds_rom_tool::le;

#[derive(Clone, Copy)]
pub struct MediaUnit(u64);

impl MediaUnit {
    pub const fn new(bytes: u64) -> Self {
        MediaUnit(bytes)
    }

    pub fn bytes(self, units: u32) -> u64 {
        u64::from(units) * self.0
    }
}

#[derive(Clone, Copy)]
pub struct Extent {
    pub offset: u32,
    pub len: u32,
}

impl Extent {
    pub fn parse(bytes: &[u8], at: usize) -> Self {
        Extent {
            offset: le::u32_at(bytes, at),
            len: le::u32_at(bytes, at + 4),
        }
    }
}
