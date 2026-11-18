use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, value_parser};

use super::apply::{Heads, Request};
use super::record::{Kind, RECORDS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Auto,
    Record(Kind),
}

#[derive(Args, Debug)]
#[command(
    about = "Write a Microsoft-compatible boot record to a device, or identify the one it has",
    after_help = "Warning: Writing the wrong kind of boot record to a device might\n\
                  destroy partition information or file system!"
)]
pub struct Invocation {
    /// Force writing of boot record
    #[arg(short, long)]
    pub force: bool,

    /// Reset partition disk label in boot record
    #[arg(short = 'l', long = "wipelabel")]
    pub wipe_label: bool,

    /// Write partition info (hidden sectors, heads and drive id) to boot
    /// record
    #[arg(short, long)]
    pub partition: bool,

    /// Manually set number of heads if partition info is written
    #[arg(short = 'H', long, value_name = "N", value_parser = value_parser!(u16).range(1..))]
    pub heads: Option<u16>,

    /// Manually set number of bytes per sector
    #[arg(
        short = 'B',
        long = "bps",
        value_name = "N",
        default_value_t = 512,
        value_parser = value_parser!(u64).range(512..=65536)
    )]
    pub bytes_per_sector: u64,

    /// Write OEM ID string to file system
    #[arg(short = 'O', long = "writeoem", value_name = "ID")]
    pub oem_id: Option<OsString>,

    /// Write Windows Disk Signature, in hexadecimal, to MBR
    #[arg(short = 'S', long = "writewds", value_name = "HEX", value_parser = disk_signature)]
    pub disk_signature: Option<u32>,

    /// Device or image file to inspect or write to
    pub device: PathBuf,

    #[command(flatten)]
    records: RecordSwitches,
}

/// One switch for each row of `RECORDS`, and the one that picks among them.
#[derive(Args, Debug)]
#[group(multiple = false)]
#[command(next_help_heading = "Boot records")]
struct RecordSwitches {
    /// Write a FAT12 floppy boot record to device
    #[arg(short = '1', long)]
    fat12: bool,
    /// Write a FAT32 partition NT5.0 boot record to device
    #[arg(short = '2', long)]
    fat32nt5: bool,
    /// Write a FAT32 partition NT6.0 boot record to device
    #[arg(short = '8', long)]
    fat32nt6: bool,
    /// Write a EXFAT partition NT6.0 boot record to device
    #[arg(short = 'x', long)]
    exfatnt6: bool,
    /// Write a FAT32 partition PE boot record to device
    #[arg(short = 'e', long)]
    fat32pe: bool,
    /// Write a FAT32 partition DOS boot record to device
    #[arg(short = '3', long)]
    fat32: bool,
    /// Write a FAT32 partition FreeDOS boot record to device
    #[arg(short = '4', long)]
    fat32free: bool,
    /// Write a FAT16 partition FreeDOS boot record to device
    #[arg(short = '5', long)]
    fat16free: bool,
    /// Write a FAT16 partition DOS boot record to device
    #[arg(short = '6', long)]
    fat16: bool,
    /// Write a NTFS partition Windows 7 boot record to device
    #[arg(short = 'n', long)]
    ntfs: bool,
    /// Write a FAT16 partition ReactOS boot record to device
    #[arg(short = 'o', long)]
    fat16ros: bool,
    /// Write a FAT32 partition ReactOS boot record to device
    #[arg(short = 'c', long)]
    fat32ros: bool,
    /// Write a FAT32 partition KolibriOS boot record to device
    #[arg(short = 'q', long)]
    fat32kos: bool,
    /// Write a Windows 7 MBR to device
    #[arg(short = '7', long)]
    mbr7: bool,
    /// Write a Windows Vista MBR to device
    #[arg(short = 'i', long)]
    mbrvista: bool,
    /// Write a Windows 2000/XP/2003 MBR to device
    #[arg(short = 'm', long)]
    mbr: bool,
    /// Write a Windows 95B/98/98SE/ME MBR to device
    #[arg(short = '9', long)]
    mbr95b: bool,
    /// Write a DOS/Windows NT MBR to device
    #[arg(short = 'd', long)]
    mbrdos: bool,
    /// Write a Syslinux MBR to device
    #[arg(short = 's', long)]
    mbrsyslinux: bool,
    /// Write a Syslinux GPT MBR to device
    #[arg(short = 't', long)]
    mbrgptsyslinux: bool,
    /// Write a ReactOS MBR to device
    #[arg(short = 'a', long)]
    mbrreactos: bool,
    /// Write a KolibriOS MBR to device
    #[arg(short = 'k', long)]
    mbrkolibrios: bool,
    /// Write a Rufus MBR to device
    #[arg(short = 'r', long)]
    mbrrufus: bool,
    /// Write a Grub4Dos MBR to device
    #[arg(short = 'g', long)]
    mbrgrub4dos: bool,
    /// Write a Grub 2 MBR to device
    #[arg(short = 'b', long)]
    mbrgrub2: bool,
    /// Write an empty (zeroed) MBR to device
    #[arg(short = 'z', long)]
    mbrzero: bool,
    /// Write automatically selected boot record to device
    #[arg(short = 'w', long)]
    write: bool,
}

impl RecordSwitches {
    fn selection(&self) -> Option<Selection> {
        if self.write {
            return Some(Selection::Auto);
        }
        RECORDS
            .iter()
            .map(|record| record.kind)
            .find(|&kind| self.given(kind))
            .map(Selection::Record)
    }

    fn given(&self, kind: Kind) -> bool {
        match kind {
            Kind::Fat12 => self.fat12,
            Kind::Fat32Nt5 => self.fat32nt5,
            Kind::Fat32Nt6 => self.fat32nt6,
            Kind::ExfatNt6 => self.exfatnt6,
            Kind::Fat32Pe => self.fat32pe,
            Kind::Fat32 => self.fat32,
            Kind::Fat32Fd => self.fat32free,
            Kind::Fat16Fd => self.fat16free,
            Kind::Fat16 => self.fat16,
            Kind::Ntfs => self.ntfs,
            Kind::Fat16Ros => self.fat16ros,
            Kind::Fat32Ros => self.fat32ros,
            Kind::Fat32Kos => self.fat32kos,
            Kind::MbrWin7 => self.mbr7,
            Kind::MbrVista => self.mbrvista,
            Kind::Mbr2000 => self.mbr,
            Kind::Mbr95b => self.mbr95b,
            Kind::MbrDos => self.mbrdos,
            Kind::MbrSyslinux => self.mbrsyslinux,
            Kind::MbrGptSyslinux => self.mbrgptsyslinux,
            Kind::MbrReactos => self.mbrreactos,
            Kind::MbrKolibrios => self.mbrkolibrios,
            Kind::MbrRufus => self.mbrrufus,
            Kind::MbrGrub4dos => self.mbrgrub4dos,
            Kind::MbrGrub2 => self.mbrgrub2,
            Kind::MbrZero => self.mbrzero,
        }
    }
}

impl Invocation {
    pub fn selection(&self) -> Option<Selection> {
        self.records.selection()
    }

    pub fn writes(&self) -> bool {
        self.selection().is_some()
            || self.partition
            || self.oem_id.is_some()
            || self.disk_signature.is_some()
    }

    pub fn request(&self, record: Option<Kind>) -> Request {
        Request {
            record,
            force: self.force,
            wipe_label: self.wipe_label,
            partition: self
                .partition
                .then_some(self.heads.map_or(Heads::Kernel, Heads::Given)),
            bytes_per_sector: self.bytes_per_sector,
            oem_id: self
                .oem_id
                .as_ref()
                .map(|id| id.as_encoded_bytes().to_vec()),
            disk_signature: self.disk_signature,
        }
    }
}

fn disk_signature(text: &str) -> Result<u32, String> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("not a hexadecimal number".to_owned());
    }
    u32::from_str_radix(digits, 16).map_err(|_| "does not fit in 32 bits".to_owned())
}
