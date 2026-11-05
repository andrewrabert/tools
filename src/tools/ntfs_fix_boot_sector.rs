//! Set the hidden-sectors field of an NTFS boot sector.

use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::tools::Tool;

/// Byte offset of the hidden-sectors field in an NTFS boot sector.
const HIDDEN_SECTORS_OFFSET: u64 = 0x1c;

#[derive(ClapArgs)]
#[command(about = "Set an NTFS boot sector's partition start, so Windows can boot from it")]
pub struct NtfsFixBootSector {
    /// NTFS partition to fix
    #[arg(value_name = "PARTITION")]
    partition: PathBuf,
    /// Start sector of the partition, as `fdisk -l DEVICE` prints it
    #[arg(value_name = "START_SECTOR")]
    start_sector: u32,
}

impl Tool for NtfsFixBootSector {
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

fn run(args: NtfsFixBootSector) -> Result<()> {
    let mut partition = OpenOptions::new()
        .write(true)
        .open(&args.partition)
        .with_context(|| format!("opening {}", args.partition.display()))?;
    partition
        .seek(SeekFrom::Start(HIDDEN_SECTORS_OFFSET))
        .and_then(|_| partition.write_all(&args.start_sector.to_le_bytes()))
        .and_then(|()| partition.sync_all())
        .with_context(|| format!("writing {}", args.partition.display()))
}
