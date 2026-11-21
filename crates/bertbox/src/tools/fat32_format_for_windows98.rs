//! Format a partition as FAT32 that Windows 98 can boot from.

use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use clap::Args as ClapArgs;
use serde::Deserialize;

use crate::tools::Tool;

/// Volume serial number written when none is given.
const DEFAULT_VOLUME_ID: u32 = 3_347_998_142;

/// The OEM name Windows 98 writes, at its offset in the boot sector.
const OEM_NAME: (u64, &[u8; 8]) = (3, b"MSWIN4.1");

/// Offset of the volume serial number in the boot sector.
const VOLUME_ID_OFFSET: u64 = 67;

/// Windows 98 boot code and messages, as base64 at their offsets in the first three sectors.
const BOOT_CODE: [(u64, &str); 20] = [
    (91, "M8mO0bz4e47BvXgAxXY="),
    (
        107,
        "VhZVvyIFiX4AiU4CsQv886SO2b0AfMZF/g+LRhiIRfk4TkB9JYvBmbsAB+iXAHIag+s6ZqEcfGY7B4pX/HUGgMoCiFYCgMMQc+2/AgCDfhYAdUWLRhyLVh65AwBJQHUBQrsAfuhfAHMmsPhPdB2LRjIz0rkD",
    ),
    (226, "yHcei3YOO85zFyvxA0YcE1Ye69FzC+sng34q"),
    (255, "A+n9Ar5+fayYA/CshMB0Fzz/dAm0DrsH"),
    (281, "EOvuvoF96+W+f33r4JjNFl4fZo8EzRlBVmZq"),
    (
        310,
        "UAZTagFqEIv0YIB+Ag51BLRC6x2RkjPS93YYkfd2GEKHyvd2GoryiujAzAIKzLgBAopWQM0TYY1kEF5yCkB1AUIDXgtJdbTDAxgBJw0KSW52YWxpZCBzeXN0ZW0gZGlza/8NCkRpc2sgSS9PIGVycm9y/w0KUmVwbGFjZSB0aGUgZGlzaywgYW5kIHRoZW4gcHJlc3MgYW55IGtleQ0K",
    ),
    (473, "TyAgICAgIFNZU01TRE9TICAgU1lTfgE="),
    (498, "SU5CT09UIFNZUw=="),
    (
        1025,
        "Zg+2RhBmi04kZvfhZgNGHGYPt1YOZgPCM8lmiUb8ZsdG+P/////6ZotGLGaD+AIPgs/8Zj34//8PD4PF/GYPpMIQ+1JQ+mbB4BBmD6zQEGaD6AJmD7ZeDYvzZvfjZgNG/GYPpMIQ+7s=",
    ),
    (1131, "i/u5AQ=="),
    (1137, "vvwPgqr8OC10HrELVr7YffOmXnQZA/mDxxU7+3LoTnXWWFroZg=="),
    (
        1176,
        "q4PEBOlk/IPEBIt1CYt9D4vG+mbB4BCLx2aD+AJyO2Y9+P//D3MzZkhmSGYPtk4NZvfhZgNG/GYPpMIQ+7s=",
    ),
    (1240, "U7kE"),
    (1245, "UvxbD4I9/IE/TVp1CIG/"),
    (1262, "Qkp0Br6AfekO/Oo="),
    (1275, "cA=="),
    (1278, "wBPSA8AT0ugY"),
    (1289, "JmaLAWYl////D2YPpMIQZj34//8P+8O/"),
    (
        1315,
        "+mbB4BBmD6zQEGYPt04LZjPSZvfxZjtG+HREZolG+GYDRhxmD7dODmYDwWYPt14og+MPdBY6XhAPg6T7UmaLyGaLRiRm9+NmA8FaUmYPpMIQ+4vfuQE=",
    ),
    (1403, "tPtaD4Kf+/uL2sM="),
];

#[derive(ClapArgs)]
#[command(about = "Format a partition as FAT32 that Windows 98 can boot from")]
pub struct Fat32FormatForWindows98 {
    /// Volume serial number to write
    #[arg(long, value_name = "ID", default_value_t = DEFAULT_VOLUME_ID)]
    vol_id: u32,
    /// Partition to format; everything on it is erased
    #[arg(value_name = "PARTITION")]
    path: PathBuf,
}

impl Tool for Fat32FormatForWindows98 {
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

fn run(args: Fat32FormatForWindows98) -> Result<()> {
    let partition = Partition::probe(&args.path)?;
    partition.format()?;
    partition.mark_bootable()?;
    partition.write_windows98_boot_sector(args.vol_id)
}

/// A partition and the disk that holds it.
struct Partition {
    path: PathBuf,
    disk: PathBuf,
    number: u32,
}

#[derive(Deserialize)]
struct Lsblk {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Deserialize)]
struct LsblkDevice {
    #[serde(rename = "type")]
    kind: String,
    pkname: Option<PathBuf>,
    partn: Option<u32>,
}

impl Partition {
    fn probe(path: &Path) -> Result<Self> {
        let output = Command::new("lsblk")
            .args([
                "--paths",
                "--json",
                "--nodeps",
                "--output",
                "type,pkname,partn",
            ])
            .arg(path)
            .stderr(Stdio::inherit())
            .output()
            .context("running lsblk")?;
        if !output.status.success() {
            bail!("lsblk failed: {}", output.status);
        }
        let lsblk: Lsblk = serde_json::from_slice(&output.stdout).context("parsing lsblk")?;
        let [device] = <[LsblkDevice; 1]>::try_from(lsblk.blockdevices)
            .map_err(|_| anyhow::anyhow!("unexpected topology for {}", path.display()))?;
        if device.kind != "part" {
            bail!("{} is not a partition", path.display());
        }
        let disk = device
            .pkname
            .with_context(|| format!("cannot determine the disk of {}", path.display()))?;
        let number = device
            .partn
            .with_context(|| format!("cannot determine the number of {}", path.display()))?;
        Ok(Self {
            path: path.to_owned(),
            disk,
            number,
        })
    }

    fn format(&self) -> Result<()> {
        status(
            Command::new("mkfs.fat")
                .args(["-F32", "-g", "240/63"])
                .arg(&self.path)
                .stdout(Stdio::null()),
            "mkfs.fat",
        )
    }

    /// Set the boot and LBA flags in the partition table.
    fn mark_bootable(&self) -> Result<()> {
        let number = self.number.to_string();
        status(
            Command::new("parted")
                .arg("--script")
                .arg(&self.disk)
                .args(["set", &number, "boot", "on", "set", &number, "lba", "on"])
                .stdout(Stdio::null()),
            "parted",
        )
    }

    fn write_windows98_boot_sector(&self, volume_id: u32) -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(&self.path)
            .with_context(|| format!("opening {}", self.path.display()))?;
        let (offset, name) = OEM_NAME;
        let mut writes = vec![
            (offset, name.to_vec()),
            (VOLUME_ID_OFFSET, volume_id.to_le_bytes().to_vec()),
        ];
        writes.extend(boot_code());
        for (offset, bytes) in writes {
            file.seek(SeekFrom::Start(offset))
                .and_then(|_| file.write_all(&bytes))
                .with_context(|| format!("writing {}", self.path.display()))?;
        }
        file.sync_all()
            .with_context(|| format!("writing {}", self.path.display()))
    }
}

/// The boot code, decoded.
fn boot_code() -> impl Iterator<Item = (u64, Vec<u8>)> {
    BOOT_CODE.iter().map(|&(offset, code)| {
        let bytes = STANDARD.decode(code).expect("boot code is valid base64");
        (offset, bytes)
    })
}

fn status(command: &mut Command, name: &str) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("running {name}"))?;
    if !status.success() {
        bail!("{name} failed: {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_code_fits_three_sectors_without_overlap() {
        let mut regions: Vec<(u64, u64)> = boot_code()
            .map(|(offset, bytes)| (offset, offset + bytes.len() as u64))
            .collect();
        regions.push((OEM_NAME.0, OEM_NAME.0 + OEM_NAME.1.len() as u64));
        regions.push((VOLUME_ID_OFFSET, VOLUME_ID_OFFSET + 4));
        regions.sort_unstable();
        for pair in regions.windows(2) {
            assert!(
                pair[0].1 <= pair[1].0,
                "{:?} overlaps {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(regions.last().is_some_and(|&(_, end)| end <= 3 * 512));
    }

    #[test]
    fn boot_code_keeps_boot_sector_signature() {
        // Bytes 510 and 511 hold the 0x55AA signature that mkfs.fat wrote.
        assert!(boot_code().all(|(offset, bytes)| {
            let end = offset + bytes.len() as u64;
            end <= 510 || offset >= 512
        }));
    }
}
