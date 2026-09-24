//! NCSD (CCI) image headers.
use std::io::{Read, Seek, SeekFrom};

use anyhow::{Context, Result, bail};

use crate::tools::three_ds_rom_tool::le;
use crate::tools::three_ds_rom_tool::media::{Extent, MediaUnit};
use crate::tools::three_ds_rom_tool::ncch::Flags;

pub const HEADER_LEN: usize = 0x1190;
const MAGIC: &[u8] = b"NCSD";
const MAGIC_AT: usize = 0x100;
const MEDIA_ID_AT: usize = 0x108;
const PARTITIONS_AT: usize = 0x120;
const PARTITION_COUNT: usize = 8;
const PARTITION_ENTRY_LEN: usize = 8;
const MEDIA_UNIT_SHIFT_AT: usize = 0x18E;
const CARD_INFO_FLAGS_AT: usize = 0x1188;
const BASE_MEDIA_UNIT: u64 = 0x200;

pub struct Ncsd {
    pub media_unit: MediaUnit,
    pub media_id: u64,
    pub card_info_flags: Flags,
    partitions: [Extent; PARTITION_COUNT],
}

impl Ncsd {
    pub fn read<R: Read + Seek>(file: &mut R) -> Result<Self> {
        let mut header = [0; HEADER_LEN];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut header)
            .context("reading the NCSD header")?;
        if &header[MAGIC_AT..MAGIC_AT + MAGIC.len()] != MAGIC {
            bail!("not a 3DS ROM (missing NCSD magic)");
        }
        let shift = header[MEDIA_UNIT_SHIFT_AT];
        let media_unit = BASE_MEDIA_UNIT
            .checked_shl(u32::from(shift))
            .with_context(|| format!("invalid media unit shift {shift}"))?;
        Ok(Ncsd {
            media_unit: MediaUnit::new(media_unit),
            media_id: le::u64_at(&header, MEDIA_ID_AT),
            card_info_flags: Flags::parse(&header[CARD_INFO_FLAGS_AT..]),
            partitions: std::array::from_fn(|index| {
                Extent::parse(&header, PARTITIONS_AT + index * PARTITION_ENTRY_LEN)
            }),
        })
    }

    pub fn partition(&self, index: usize) -> Option<Extent> {
        let extent = self.partitions[index];
        (extent.offset != 0).then_some(extent)
    }

    pub fn partitions(&self) -> impl Iterator<Item = Option<Extent>> {
        (0..PARTITION_COUNT).map(|index| self.partition(index))
    }
}
