use std::io;

use super::device::Device;
use super::exfat;
use super::payloads as p;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    MbrDos = 2,
    Fat12 = 3,
    Fat16 = 4,
    Fat32 = 5,
    Fat32Nt5 = 6,
    Fat32Nt6 = 7,
    ExfatNt6 = 8,
    Mbr95b = 9,
    Mbr2000 = 10,
    MbrVista = 11,
    MbrWin7 = 12,
    MbrSyslinux = 13,
    MbrZero = 14,
    Fat16Fd = 15,
    Fat32Fd = 16,
    Fat32Pe = 17,
    Ntfs = 18,
    MbrGptSyslinux = 19,
    MbrRufus = 20,
    MbrReactos = 21,
    Fat16Ros = 22,
    Fat32Ros = 23,
    Fat32Kos = 24,
    MbrGrub4dos = 25,
    MbrGrub2 = 26,
    MbrKolibrios = 27,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Mbr,
    Fat12,
    Fat16,
    Fat32,
    Ntfs,
    Exfat,
}

impl Family {
    pub fn file_system(self) -> &'static str {
        match self {
            Self::Mbr => "",
            Self::Fat12 => "FAT12",
            Self::Fat16 => "FAT16",
            Self::Fat32 => "FAT32",
            Self::Ntfs => "NTFS",
            Self::Exfat => "EXFAT",
        }
    }

    fn label_offset(self) -> Option<u64> {
        match self {
            Self::Fat12 | Self::Fat16 => Some(0x2b),
            Self::Fat32 => Some(0x47),
            Self::Mbr | Self::Ntfs | Self::Exfat => None,
        }
    }
}

pub struct Record {
    pub kind: Kind,
    pub family: Family,
    pub short: char,
    /// The long switch; only the tests read it, to hold the parser to it.
    #[cfg_attr(not(test), expect(dead_code))]
    pub long: &'static str,
    pub written: &'static str,
    /// Name used in "Failed writing … to …". Upstream spells two of these
    /// differently from `written`.
    pub failed: &'static str,
    pub described: &'static str,
    pub segments: &'static [(u64, &'static [u8])],
}

pub static RECORDS: [Record; 26] = [
    Record {
        kind: Kind::MbrDos,
        family: Family::Mbr,
        short: 'd',
        long: "mbrdos",
        written: "DOS/Windows NT master boot record",
        failed: "DOS/Windows NT master boot record",
        described: "Microsoft DOS/NT/95A",
        segments: &[(0x0, p::MBR_DOS)],
    },
    Record {
        kind: Kind::Fat12,
        family: Family::Fat12,
        short: '1',
        long: "fat12",
        written: "FAT12 boot record",
        failed: "FAT12 boot record",
        described: "FAT12",
        segments: &[(0x0, p::BR_FAT12_0X0), (0x3e, p::BR_FAT12_0X3E)],
    },
    Record {
        kind: Kind::Fat16,
        family: Family::Fat16,
        short: '6',
        long: "fat16",
        written: "FAT16 boot record",
        failed: "FAT16 boot record",
        described: "FAT16 DOS",
        segments: &[(0x0, p::BR_FAT16_0X0), (0x3e, p::BR_FAT16_0X3E)],
    },
    Record {
        kind: Kind::Fat32,
        family: Family::Fat32,
        short: '3',
        long: "fat32",
        written: "FAT32 DOS boot record",
        failed: "FAT32 DOS boot record",
        described: "FAT32 DOS",
        segments: &[
            (0x0, p::BR_FAT32_0X0),
            (0x52, p::BR_FAT32_0X52),
            (0x3f0, p::BR_FAT32_0X3F0),
        ],
    },
    Record {
        kind: Kind::Fat32Nt5,
        family: Family::Fat32,
        short: '2',
        long: "fat32nt5",
        written: "FAT32 NT5.0 boot record",
        failed: "FAT32 NT5.0 boot record",
        described: "FAT32 NT5.0",
        segments: &[
            (0x0, p::BR_FAT32_0X0),
            (0x52, p::BR_FAT32NT5_0X52),
            (0x3f0, p::BR_FAT32NT5_0X3F0),
            (0x1800, p::BR_FAT32NT5_0X1800),
        ],
    },
    Record {
        kind: Kind::Fat32Nt6,
        family: Family::Fat32,
        short: '8',
        long: "fat32nt6",
        written: "FAT32 NT6.0 boot record",
        failed: "FAT32 NT6.0 boot record",
        described: "FAT32 NT6.0",
        segments: &[
            (0x0, p::BR_FAT32NT6_0X0),
            (0x52, p::BR_FAT32NT6_0X52),
            (0x5fe, p::BR_FAT32NT6_0X5FE),
            (0x1800, p::BR_FAT32NT6_0X1800),
        ],
    },
    Record {
        kind: Kind::ExfatNt6,
        family: Family::Exfat,
        short: 'x',
        long: "exfatnt6",
        written: "EXFAT NT6.0 boot record",
        failed: "EXFAT NT6.0 boot record",
        described: "EXFAT NT6.0",
        segments: &[],
    },
    Record {
        kind: Kind::Mbr95b,
        family: Family::Mbr,
        short: '9',
        long: "mbr95b",
        written: "Windows 95B/98/98SE/ME master boot record",
        failed: "Windows 95B/98/98SE/ME master boot record",
        described: "Microsoft 95B/98/98SE/ME",
        segments: &[(0x0, p::MBR_95B_0X0), (0x0e0, p::MBR_95B_0X0E0)],
    },
    Record {
        kind: Kind::Mbr2000,
        family: Family::Mbr,
        short: 'm',
        long: "mbr",
        written: "Windows 2000/XP/2003 master boot record",
        failed: "Windows 2000/XP/2003 master boot record",
        described: "Microsoft 2000/XP/2003",
        segments: &[(0x0, p::MBR_2000)],
    },
    Record {
        kind: Kind::MbrVista,
        family: Family::Mbr,
        short: 'i',
        long: "mbrvista",
        written: "Windows Vista master boot record",
        failed: "Windows Vista master boot record",
        described: "Microsoft Vista",
        segments: &[(0x0, p::MBR_VISTA)],
    },
    Record {
        kind: Kind::MbrWin7,
        family: Family::Mbr,
        short: '7',
        long: "mbr7",
        written: "Windows 7 master boot record",
        failed: "Windows 7 master boot record",
        described: "Microsoft 7",
        segments: &[(0x0, p::MBR_WIN7)],
    },
    Record {
        kind: Kind::MbrSyslinux,
        family: Family::Mbr,
        short: 's',
        long: "mbrsyslinux",
        written: "Syslinux master boot record",
        failed: "Syslinux master boot record",
        described: "public domain syslinux",
        segments: &[(0x0, p::MBR_SYSLINUX)],
    },
    Record {
        kind: Kind::MbrZero,
        family: Family::Mbr,
        short: 'z',
        long: "mbrzero",
        written: "Empty (zeroed) master boot record",
        failed: "empty (zeroed) master boot record",
        described: "",
        segments: &[(0x0, p::MBR_ZERO)],
    },
    Record {
        kind: Kind::Fat16Fd,
        family: Family::Fat16,
        short: '5',
        long: "fat16free",
        written: "FAT16 FreeDOS boot record",
        failed: "FAT16 FreeDOS boot record",
        described: "FAT16 FreeDOS",
        segments: &[(0x0, p::BR_FAT16_0X0), (0x3e, p::BR_FAT16FD_0X3E)],
    },
    Record {
        kind: Kind::Fat32Fd,
        family: Family::Fat32,
        short: '4',
        long: "fat32free",
        written: "FAT32 FreeDOS boot record",
        failed: "FAT32 FreeDOS boot record",
        described: "FAT32 FreeDOS",
        segments: &[
            (0x0, p::BR_FAT32_0X0),
            (0x52, p::BR_FAT32FD_0X52),
            (0x3f0, p::BR_FAT32FD_0X3F0),
        ],
    },
    Record {
        kind: Kind::Fat32Pe,
        family: Family::Fat32,
        short: 'e',
        long: "fat32pe",
        written: "FAT32 PE boot record",
        failed: "FAT32 PE boot record",
        described: "FAT32 PE",
        segments: &[
            (0x0, p::BR_FAT32_0X0),
            (0x52, p::BR_FAT32PE_0X52),
            (0x3f0, p::BR_FAT32PE_0X3F0),
            (0x1800, p::BR_FAT32PE_0X1800),
        ],
    },
    Record {
        kind: Kind::Ntfs,
        family: Family::Ntfs,
        short: 'n',
        long: "ntfs",
        written: "NTFS Windows 7 boot record",
        failed: "NTFS Windows 7 boot record",
        described: "NTFS",
        segments: &[(0x0, p::BR_NTFS_0X0), (0x54, p::BR_NTFS_0X54)],
    },
    Record {
        kind: Kind::MbrGptSyslinux,
        family: Family::Mbr,
        short: 't',
        long: "mbrgptsyslinux",
        written: "Syslinux GPT master boot record",
        failed: "Syslinux GPT master boot record",
        described: "GPL syslinux GPT",
        segments: &[(0x0, p::MBR_GPT_SYSLINUX)],
    },
    Record {
        kind: Kind::MbrRufus,
        family: Family::Mbr,
        short: 'r',
        long: "mbrrufus",
        written: "Rufus master boot record",
        failed: "Rufus master boot record",
        described: "Rufus",
        segments: &[(0x0, p::MBR_RUFUS)],
    },
    Record {
        kind: Kind::MbrReactos,
        family: Family::Mbr,
        short: 'a',
        long: "mbrreactos",
        written: "ReactOS master boot record",
        failed: "ReactOS master boot record",
        described: "ReactOS",
        segments: &[(0x0, p::MBR_REACTOS)],
    },
    Record {
        kind: Kind::Fat16Ros,
        family: Family::Fat16,
        short: 'o',
        long: "fat16ros",
        written: "FAT16 ReactOS boot record",
        failed: "FAT16 ReactOS boot record",
        described: "FAT16 ReactOS",
        segments: &[(0x0, p::BR_FAT16ROS_0X0), (0x3e, p::BR_FAT16ROS_0X3E)],
    },
    Record {
        kind: Kind::Fat32Ros,
        family: Family::Fat32,
        short: 'c',
        long: "fat32ros",
        written: "FAT32 ReactOS boot record",
        failed: "FAT32 ReactOS boot record",
        described: "FAT32 ReactOS",
        segments: &[
            (0x0, p::BR_FAT32_0X0),
            (0x52, p::BR_FAT32ROS_0X52),
            (0x3f0, p::BR_FAT32ROS_0X3F0),
            (0x1c00, p::BR_FAT32ROS_0X1C00),
        ],
    },
    Record {
        kind: Kind::Fat32Kos,
        family: Family::Fat32,
        short: 'q',
        long: "fat32kos",
        written: "FAT32 KolibriOS boot record",
        failed: "FAT32 KolibriOS boot record",
        described: "FAT32 KolibriOS",
        segments: &[(0x0, p::BR_FAT32_0X0), (0x52, p::BR_FAT32KOS_0X52)],
    },
    Record {
        kind: Kind::MbrGrub4dos,
        family: Family::Mbr,
        short: 'g',
        long: "mbrgrub4dos",
        written: "Grub4DOS master boot record",
        failed: "Grub4DOS master boot record",
        described: "Grub4DOS",
        segments: &[(0x0, p::MBR_GRUB)],
    },
    Record {
        kind: Kind::MbrGrub2,
        family: Family::Mbr,
        short: 'b',
        long: "mbrgrub2",
        written: "GRUB 2 master boot record",
        failed: "GRUB2 master boot record",
        described: "GRUB 2",
        segments: &[(0x0, p::MBR_GRUB2)],
    },
    Record {
        kind: Kind::MbrKolibrios,
        family: Family::Mbr,
        short: 'k',
        long: "mbrkolibrios",
        written: "KolibriOS master boot record",
        failed: "KolibriOS master boot record",
        described: "KolibriOS",
        segments: &[(0x0, p::MBR_KOLIBRI)],
    },
];

impl Kind {
    pub fn record(self) -> &'static Record {
        &RECORDS[self as usize - Kind::MbrDos as usize]
    }
}

pub fn has_boot_signature(device: &dyn Device) -> bool {
    device.contains(0x1fe, &[0x55, 0xaa])
}

/// UPSTREAM-BUG: `--bps` only changes how many signatures are written here.
/// Nothing else in the program honours it, including the exFAT code, which
/// asks the kernel for the sector size.
fn write_boot_signatures(device: &mut dyn Device, bytes_per_sector: u64) -> io::Result<()> {
    (0x1fe..bytes_per_sector)
        .step_by(0x200)
        .try_for_each(|pos| device.write_at(pos, &[0x55, 0xaa]))
}

impl Record {
    pub fn write(
        &self,
        device: &mut dyn Device,
        keep_label: bool,
        bytes_per_sector: u64,
    ) -> io::Result<()> {
        if self.family == Family::Exfat {
            return exfat::write_boot_record(device);
        }
        // The label goes between the first array and the rest.
        let label = self.family.label_offset().filter(|_| !keep_label);
        let mut segments = self.segments.iter();
        if let Some(&(pos, data)) = segments.next() {
            device.write_at(pos, data)?;
        }
        if let Some(pos) = label {
            device.write_at(pos, p::LABEL_11_CHAR)?;
        }
        for &(pos, data) in segments {
            device.write_at(pos, data)?;
        }
        if self.family == Family::Mbr {
            write_boot_signatures(device, bytes_per_sector)?;
        }
        Ok(())
    }

    pub fn matches(&self, device: &dyn Device) -> bool {
        if self.family == Family::Exfat {
            return exfat::boot_record_matches(device);
        }
        // Upstream's `is_zero_mbr` is the one MBR check that skips the boot
        // signature. Its only caller has already found the signature, so
        // this never shows in the output.
        self.segments
            .iter()
            .all(|&(pos, data)| device.contains(pos, data))
            && (self.family != Family::Mbr
                || self.kind == Kind::MbrZero
                || has_boot_signature(device))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_in_record_type_order() {
        for (index, record) in RECORDS.iter().enumerate() {
            assert_eq!(record.kind as usize, index + 2);
            assert_eq!(record.kind.record().long, record.long);
        }
    }

    #[test]
    fn switches_are_unique() {
        for (index, record) in RECORDS.iter().enumerate() {
            for other in &RECORDS[index + 1..] {
                assert_ne!(record.short, other.short);
                assert_ne!(record.long, other.long);
            }
        }
    }

    #[test]
    fn segments_do_not_overlap() {
        for record in &RECORDS {
            let mut end = 0;
            for &(pos, data) in record.segments {
                assert!(pos >= end, "{} overlaps at {pos:#x}", record.long);
                end = pos + data.len() as u64;
            }
        }
    }
}
