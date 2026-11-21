use std::fmt;

use super::device::Device;
use super::exfat;
use super::payloads::{MBR_DOS_F2, MBR_ZERO};
use super::record::{Family, Kind, RECORDS, has_boot_signature};

pub const DISK_SIGNATURE_OFFSET: u64 = 0x1b8;
pub const OEM_ID_OFFSET: u64 = 0x3;

const FAT_ORDER: [Kind; 10] = [
    Kind::Fat16,
    Kind::Fat16Fd,
    Kind::Fat16Ros,
    Kind::Fat32,
    Kind::Fat32Nt5,
    Kind::Fat32Nt6,
    Kind::Fat32Pe,
    Kind::Fat32Fd,
    Kind::Fat32Ros,
    Kind::Fat32Kos,
];

const MBR_ORDER: [Kind; 11] = [
    Kind::Mbr95b,
    Kind::Mbr2000,
    Kind::MbrVista,
    Kind::MbrWin7,
    Kind::MbrSyslinux,
    Kind::MbrGptSyslinux,
    Kind::MbrRufus,
    Kind::MbrReactos,
    Kind::MbrKolibrios,
    Kind::MbrGrub4dos,
    Kind::MbrGrub2,
];

pub fn is_fat12_fs(device: &dyn Device) -> bool {
    device.contains(0x36, b"FAT12   ")
}

pub fn is_fat16_fs(device: &dyn Device) -> bool {
    device.contains(0x36, b"FAT16   ")
}

pub fn is_fat32_fs(device: &dyn Device) -> bool {
    device.contains(0x52, b"FAT32   ")
}

pub fn is_ntfs_fs(device: &dyn Device) -> bool {
    device.contains(0x3, b"NTFS    ")
}

fn has_file_system(device: &dyn Device, family: Family) -> bool {
    match family {
        Family::Mbr => false,
        Family::Fat12 => is_fat12_fs(device),
        Family::Fat16 => is_fat16_fs(device),
        Family::Fat32 => is_fat32_fs(device),
        Family::Ntfs => is_ntfs_fs(device),
        Family::Exfat => exfat::is_file_system(device),
    }
}

fn is_fat16_br(device: &dyn Device) -> bool {
    has_boot_signature(device) && device.contains(0x3, b"MSWIN4.1")
}

fn is_fat32_br(device: &dyn Device) -> bool {
    [0x1fe, 0x3fe, 0x5fe]
        .iter()
        .all(|&pos| device.contains(pos, &[0x55, 0xaa]))
        && (device.contains(0x3, b"MSWIN4.1") || device.contains(0x3, b"MSDOS5.0"))
}

fn is_lilo_br(device: &dyn Device) -> bool {
    device.contains(0x6, b"LILO") || device.contains(0x2, b"LILO")
}

fn read_disk_signature(device: &dyn Device) -> u32 {
    let mut bytes = [0; 4];
    if device.read_at(DISK_SIGNATURE_OFFSET, &mut bytes).is_ok() {
        u32::from_le_bytes(bytes)
    } else {
        0
    }
}

fn read_copy_protect_bytes(device: &dyn Device) -> u16 {
    let mut bytes = [0; 2];
    if device.read_at(0x1bc, &mut bytes).is_ok() {
        u16::from_le_bytes(bytes)
    } else {
        0xffff
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OemId(pub [u8; 8]);

impl fmt::Debug for OemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OemId(\"{}\")", self.0.escape_ascii())
    }
}

fn read_oem_id(device: &dyn Device) -> Option<OemId> {
    let mut id = [0; 8];
    device.read_at(OEM_ID_OFFSET, &mut id).ok()?;
    // UPSTREAM-BUG: upstream compares plain `char`s with ' ', and `char` is
    // signed on x86, so bytes 0x80 and above count as unprintable while 0x7f
    // (DEL) passes.
    id.iter()
        .all(|&byte| byte.cast_signed() >= b' '.cast_signed())
        .then_some(OemId(id))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsuitable {
    NotDiskDevice,
    Floppy,
    Partition,
    NotFloppy,
    NotPartition,
    NoFileSystem,
}

pub fn sanity_check(device: &dyn Device, kind: Kind) -> Result<(), Unsuitable> {
    let probe = device.probe();
    let family = kind.record().family;
    let on_right_device = match family {
        Family::Mbr => {
            return if !probe.is_disk_device() {
                Err(Unsuitable::NotDiskDevice)
            } else if probe.floppy {
                Err(Unsuitable::Floppy)
            } else if probe.is_partition() {
                Err(Unsuitable::Partition)
            } else {
                Ok(())
            };
        }
        Family::Fat12 => probe.floppy.then_some(()).ok_or(Unsuitable::NotFloppy),
        _ => probe
            .is_partition()
            .then_some(())
            .ok_or(Unsuitable::NotPartition),
    };
    on_right_device?;
    if has_file_system(device, family) {
        Ok(())
    } else {
        Err(Unsuitable::NoFileSystem)
    }
}

pub fn smart_select(device: &dyn Device) -> Option<Kind> {
    RECORDS
        .iter()
        .map(|record| record.kind)
        .find(|&kind| sanity_check(device, kind).is_ok())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OemIdUnsuitable {
    WrongDevice,
    NoFatFileSystem,
}

/// UPSTREAM-BUG: only FAT12, FAT16 and FAT32 pass, although NTFS and exFAT
/// keep an OEM ID at the same offset.
pub fn ok_to_write_oem_id(device: &dyn Device) -> Result<(), OemIdUnsuitable> {
    if [Kind::Fat12, Kind::Fat16, Kind::Fat32]
        .iter()
        .any(|&kind| sanity_check(device, kind).is_ok())
    {
        Ok(())
    } else if is_fat12_fs(device) || is_fat16_fs(device) || is_fat32_fs(device) {
        Err(OemIdUnsuitable::WrongDevice)
    } else {
        Err(OemIdUnsuitable::NoFatFileSystem)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootRecord {
    Exact(Kind),
    Mbr {
        kind: Kind,
        disk_signature: u32,
    },
    MbrDosF2 {
        disk_signature: u32,
    },
    MbrZeroWith {
        disk_signature: u32,
        copy_protect: u16,
    },
    ForeignFat,
    ForeignExfat,
    Lilo,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootSector {
    pub record: BootRecord,
    pub oem_id: Option<OemId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnosis {
    pub file_systems: Vec<Family>,
    pub boot_sector: Option<BootSector>,
}

pub fn inspect(device: &dyn Device) -> Diagnosis {
    let file_systems = [
        Family::Fat12,
        Family::Fat16,
        Family::Fat32,
        Family::Ntfs,
        Family::Exfat,
    ]
    .into_iter()
    .filter(|&family| has_file_system(device, family))
    .collect();
    let boot_sector = has_boot_signature(device).then(|| BootSector {
        record: boot_record(device),
        oem_id: read_oem_id(device),
    });
    Diagnosis {
        file_systems,
        boot_sector,
    }
}

fn boot_record(device: &dyn Device) -> BootRecord {
    let matches = |kind: Kind| kind.record().matches(device);
    let mbr = |kind: Kind| BootRecord::Mbr {
        kind,
        disk_signature: read_disk_signature(device),
    };

    if matches(Kind::Ntfs) {
        BootRecord::Exact(Kind::Ntfs)
    } else if matches(Kind::ExfatNt6) {
        BootRecord::Exact(Kind::ExfatNt6)
    } else if matches(Kind::Fat12) {
        BootRecord::Exact(Kind::Fat12)
    } else if is_fat16_br(device) || is_fat32_br(device) {
        FAT_ORDER
            .into_iter()
            .find(|&kind| matches(kind))
            .map_or(BootRecord::ForeignFat, BootRecord::Exact)
    } else if exfat::is_boot_record(device) {
        BootRecord::ForeignExfat
    } else if is_lilo_br(device) {
        BootRecord::Lilo
    } else if matches(Kind::MbrDos) {
        mbr(Kind::MbrDos)
    } else if device.contains(0x0, MBR_DOS_F2) {
        BootRecord::MbrDosF2 {
            disk_signature: read_disk_signature(device),
        }
    } else if let Some(kind) = MBR_ORDER.into_iter().find(|&kind| matches(kind)) {
        mbr(kind)
    } else if matches(Kind::MbrZero) {
        BootRecord::Exact(Kind::MbrZero)
    } else if device.contains(0x0, &MBR_ZERO[..DISK_SIGNATURE_OFFSET as usize]) {
        BootRecord::MbrZeroWith {
            disk_signature: read_disk_signature(device),
            copy_protect: read_copy_protect_bytes(device),
        }
    } else {
        BootRecord::Unknown
    }
}
