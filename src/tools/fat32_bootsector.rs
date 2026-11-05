//! Print the BIOS parameter block of a FAT32 boot sector.

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use serde::Serialize;

use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "Print the fields of a FAT32 boot sector as JSON")]
pub struct Fat32Bootsector {
    /// Partition, device, or image whose first sector is a FAT32 boot sector
    #[arg(value_name = "PATH")]
    path: PathBuf,
}

impl Tool for Fat32Bootsector {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: Fat32Bootsector) -> Result<()> {
    let mut sector = [0; BootSector::LEN];
    File::open(&args.path)
        .and_then(|mut file| file.read_exact(&mut sector))
        .with_context(|| format!("reading {}", args.path.display()))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&BootSector::parse(&sector))?
    );
    Ok(())
}

/// The fields of a FAT32 boot sector, named as in Microsoft's FAT specification.
///
/// Binary fields are hex; text fields are ASCII.
#[derive(Debug, PartialEq, Eq, Serialize)]
struct BootSector {
    #[serde(rename = "BS_jmpBoot")]
    bs_jmp_boot: String,
    #[serde(rename = "BS_OEMName")]
    bs_oem_name: String,
    #[serde(rename = "BPB_BytsPerSec")]
    bpb_byts_per_sec: u16,
    #[serde(rename = "BPB_SecPerClus")]
    bpb_sec_per_clus: u8,
    #[serde(rename = "BPB_RsvdSecCnt")]
    bpb_rsvd_sec_cnt: u16,
    #[serde(rename = "BPB_NumFATs")]
    bpb_num_fats: u8,
    #[serde(rename = "BPB_RootEntCnt")]
    bpb_root_ent_cnt: u16,
    #[serde(rename = "BPB_TotSec16")]
    bpb_tot_sec16: u16,
    #[serde(rename = "BPB_Media")]
    bpb_media: u8,
    #[serde(rename = "BPB_FATSz16")]
    bpb_fat_sz16: u16,
    #[serde(rename = "BPB_SecPerTrk")]
    bpb_sec_per_trk: u16,
    #[serde(rename = "BPB_NumHeads")]
    bpb_num_heads: u16,
    #[serde(rename = "BPB_HiddSec")]
    bpb_hidd_sec: u32,
    #[serde(rename = "BPB_TotSec32")]
    bpb_tot_sec32: u32,
    #[serde(rename = "BPB_FATSz32")]
    bpb_fat_sz32: u32,
    #[serde(rename = "BPB_ExtFlags")]
    bpb_ext_flags: u16,
    #[serde(rename = "BPB_FSVer")]
    bpb_fs_ver: u16,
    #[serde(rename = "BPB_RootClus")]
    bpb_root_clus: u32,
    #[serde(rename = "BPB_FSInfo")]
    bpb_fs_info: u16,
    #[serde(rename = "BPB_BkBootSec")]
    bpb_bk_boot_sec: u16,
    #[serde(rename = "BPB_Reserved")]
    bpb_reserved: String,
    #[serde(rename = "BS_DrvNum")]
    bs_drv_num: u8,
    #[serde(rename = "BS_Reserved1")]
    bs_reserved1: u8,
    #[serde(rename = "BS_BootSig")]
    bs_boot_sig: u8,
    #[serde(rename = "BS_VolID")]
    bs_vol_id: u32,
    #[serde(rename = "BS_VolLab")]
    bs_vol_lab: String,
    #[serde(rename = "BS_FilSysType")]
    bs_fil_sys_type: String,
}

impl BootSector {
    /// Bytes from the start of the sector through `BS_FilSysType`.
    const LEN: usize = 90;

    fn parse(sector: &[u8; Self::LEN]) -> Self {
        let mut fields = Fields { sector, offset: 0 };
        Self {
            bs_jmp_boot: hex::encode(fields.bytes(3)),
            bs_oem_name: String::from_utf8_lossy(fields.bytes(8)).into_owned(),
            bpb_byts_per_sec: fields.u16(),
            bpb_sec_per_clus: fields.u8(),
            bpb_rsvd_sec_cnt: fields.u16(),
            bpb_num_fats: fields.u8(),
            bpb_root_ent_cnt: fields.u16(),
            bpb_tot_sec16: fields.u16(),
            bpb_media: fields.u8(),
            bpb_fat_sz16: fields.u16(),
            bpb_sec_per_trk: fields.u16(),
            bpb_num_heads: fields.u16(),
            bpb_hidd_sec: fields.u32(),
            bpb_tot_sec32: fields.u32(),
            bpb_fat_sz32: fields.u32(),
            bpb_ext_flags: fields.u16(),
            bpb_fs_ver: fields.u16(),
            bpb_root_clus: fields.u32(),
            bpb_fs_info: fields.u16(),
            bpb_bk_boot_sec: fields.u16(),
            bpb_reserved: hex::encode(fields.bytes(12)),
            bs_drv_num: fields.u8(),
            bs_reserved1: fields.u8(),
            bs_boot_sig: fields.u8(),
            bs_vol_id: fields.u32(),
            bs_vol_lab: String::from_utf8_lossy(fields.bytes(11)).into_owned(),
            bs_fil_sys_type: String::from_utf8_lossy(fields.bytes(8)).into_owned(),
        }
    }
}

/// Reads consecutive little-endian fields from a boot sector.
struct Fields<'a> {
    sector: &'a [u8; BootSector::LEN],
    offset: usize,
}

impl<'a> Fields<'a> {
    fn bytes(&mut self, len: usize) -> &'a [u8] {
        let field = &self.sector[self.offset..self.offset + len];
        self.offset += len;
        field
    }

    fn array<const N: usize>(&mut self) -> [u8; N] {
        let mut array = [0; N];
        array.copy_from_slice(self.bytes(N));
        array
    }

    fn u8(&mut self) -> u8 {
        u8::from_le_bytes(self.array())
    }

    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.array())
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.array())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mkfs_fat_boot_sector() {
        let mut sector = [0; BootSector::LEN];
        sector[..3].copy_from_slice(&[0xeb, 0x58, 0x90]);
        sector[3..11].copy_from_slice(b"mkfs.fat");
        sector[11..13].copy_from_slice(&512u16.to_le_bytes());
        sector[13] = 8;
        sector[14..16].copy_from_slice(&32u16.to_le_bytes());
        sector[16] = 2;
        sector[21] = 0xf8;
        sector[24..26].copy_from_slice(&63u16.to_le_bytes());
        sector[26..28].copy_from_slice(&240u16.to_le_bytes());
        sector[28..32].copy_from_slice(&2048u32.to_le_bytes());
        sector[32..36].copy_from_slice(&1_048_576u32.to_le_bytes());
        sector[36..40].copy_from_slice(&1022u32.to_le_bytes());
        sector[44..48].copy_from_slice(&2u32.to_le_bytes());
        sector[48..50].copy_from_slice(&1u16.to_le_bytes());
        sector[50..52].copy_from_slice(&6u16.to_le_bytes());
        sector[64] = 0x80;
        sector[66] = 0x29;
        sector[67..71].copy_from_slice(&3_347_998_142u32.to_le_bytes());
        sector[71..82].copy_from_slice(b"NO NAME    ");
        sector[82..90].copy_from_slice(b"FAT32   ");

        let parsed = BootSector::parse(&sector);

        assert_eq!(parsed.bs_jmp_boot, "eb5890");
        assert_eq!(parsed.bs_oem_name, "mkfs.fat");
        assert_eq!(parsed.bpb_byts_per_sec, 512);
        assert_eq!(parsed.bpb_sec_per_clus, 8);
        assert_eq!(parsed.bpb_rsvd_sec_cnt, 32);
        assert_eq!(parsed.bpb_num_fats, 2);
        assert_eq!(parsed.bpb_media, 0xf8);
        assert_eq!(parsed.bpb_sec_per_trk, 63);
        assert_eq!(parsed.bpb_num_heads, 240);
        assert_eq!(parsed.bpb_hidd_sec, 2048);
        assert_eq!(parsed.bpb_tot_sec32, 1_048_576);
        assert_eq!(parsed.bpb_fat_sz32, 1022);
        assert_eq!(parsed.bpb_root_clus, 2);
        assert_eq!(parsed.bpb_fs_info, 1);
        assert_eq!(parsed.bpb_bk_boot_sec, 6);
        assert_eq!(parsed.bpb_reserved, "000000000000000000000000");
        assert_eq!(parsed.bs_drv_num, 0x80);
        assert_eq!(parsed.bs_boot_sig, 0x29);
        assert_eq!(parsed.bs_vol_id, 3_347_998_142);
        assert_eq!(parsed.bs_vol_lab, "NO NAME    ");
        assert_eq!(parsed.bs_fil_sys_type, "FAT32   ");
    }

    #[test]
    fn json_keeps_specification_names_in_sector_order() -> Result<()> {
        let json = serde_json::to_string(&BootSector::parse(&[0; BootSector::LEN]))?;
        assert!(json.starts_with(r#"{"BS_jmpBoot":"000000","BS_OEMName":"#));
        assert!(
            json.ends_with(
                r#""BS_FilSysType":"\u0000\u0000\u0000\u0000\u0000\u0000\u0000\u0000"}"#
            )
        );
        Ok(())
    }
}
