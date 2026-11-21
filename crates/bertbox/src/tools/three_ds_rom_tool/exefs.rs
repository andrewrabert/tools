//! The ExeFS header's file table.
use crate::tools::three_ds_rom_tool::le;

pub const HEADER_LEN: u64 = 0x200;
const ENTRY_LEN: usize = 0x10;
const NAME_LEN: usize = 8;

pub struct ExefsFile {
    pub offset: u64,
    pub len: u64,
}

/// Offsets are relative to the end of the header.
pub fn find(header: &[u8], name: &[u8]) -> Option<ExefsFile> {
    header.as_chunks::<ENTRY_LEN>().0.iter().find_map(|entry| {
        let entry_name = &entry[..NAME_LEN];
        let name_end = entry_name
            .iter()
            .rposition(|&b| b != 0)
            .map_or(0, |i| i + 1);
        (&entry_name[..name_end] == name).then(|| ExefsFile {
            offset: u64::from(le::u32_at(entry, NAME_LEN)),
            len: u64::from(le::u32_at(entry, NAME_LEN + 4)),
        })
    })
}
