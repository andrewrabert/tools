use std::io;

use super::device::Device;
use super::payloads::BR_EXFATNT6_0X78;

const BOOT_CODE_OFFSET: u64 = 0x78;

pub fn is_file_system(device: &dyn Device) -> bool {
    device.contains(0x3, b"EXFAT   ")
}

pub fn is_boot_record(device: &dyn Device) -> bool {
    let sector = device.probe().sector_size_or_default();
    // Wrapping, as upstream's unsigned arithmetic does for a zero sector
    // size; the resulting offset is unreadable and the check fails.
    (0..9u64).all(|i| {
        let pos = sector.wrapping_sub(2).wrapping_add(i.wrapping_mul(sector));
        device.contains(pos, &[0x55, 0xaa])
    }) && is_file_system(device)
}

fn backup_boot_code_offset(device: &dyn Device) -> u64 {
    device
        .probe()
        .sector_size_or_default()
        .wrapping_mul(12)
        .wrapping_add(BOOT_CODE_OFFSET)
}

pub fn boot_record_matches(device: &dyn Device) -> bool {
    device.contains(BOOT_CODE_OFFSET, BR_EXFATNT6_0X78)
        && device.contains(backup_boot_code_offset(device), BR_EXFATNT6_0X78)
}

pub fn write_boot_record(device: &mut dyn Device) -> io::Result<()> {
    device.write_at(BOOT_CODE_OFFSET, BR_EXFATNT6_0X78)?;
    device.write_at(backup_boot_code_offset(device), BR_EXFATNT6_0X78)
}

pub fn boot_checksum(sectors: &[u8]) -> u32 {
    sectors
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != 0x6a && i != 0x6b && i != 0x70)
        .fold(0u32, |sum, (_, &byte)| {
            sum.rotate_right(1).wrapping_add(u32::from(byte))
        })
}

pub fn write_boot_checksum(device: &mut dyn Device) -> io::Result<()> {
    let sector = device.probe().sector_size_or_default();
    let length = sector
        .checked_mul(11)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or(io::ErrorKind::InvalidInput)?;
    // Upstream's `fread` of eleven zero-sized items reads nothing and fails.
    if length == 0 {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    let mut sectors = Vec::new();
    sectors
        .try_reserve_exact(length)
        .map_err(|_| io::ErrorKind::OutOfMemory)?;
    sectors.resize(length, 0);
    device.read_at(0, &mut sectors)?;
    let checksum = boot_checksum(&sectors).to_le_bytes();
    let filled = checksum.repeat(length / 11 / checksum.len());
    // A sector too small to hold one checksum gets nothing written, which
    // upstream counts as success; so does an empty write here.
    device.write_at(sector * 11, &filled)?;
    device.write_at(sector * 23, &filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_skips_volatile_fields() {
        let mut sectors = vec![0u8; 512 * 11];
        sectors[0] = 1;
        let baseline = boot_checksum(&sectors);
        for volatile in [0x6a, 0x6b, 0x70] {
            sectors[volatile] = 0xff;
        }
        assert_eq!(boot_checksum(&sectors), baseline);
        sectors[0x6c] = 0xff;
        assert_ne!(boot_checksum(&sectors), baseline);
    }

    #[test]
    fn checksum_rotates_right_and_adds() {
        assert_eq!(boot_checksum(&[1]), 1);
        assert_eq!(boot_checksum(&[1, 0]), 0x8000_0000);
        assert_eq!(boot_checksum(&[1, 2]), 0x8000_0002);
        assert_eq!(boot_checksum(&[0xff, 0xff, 0xff]), 0x4000_01be);
    }
}
