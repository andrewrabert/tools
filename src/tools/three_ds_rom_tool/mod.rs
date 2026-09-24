//! CLI dispatch for 3DS ROM decryption, encryption, and CIA conversion.
mod cia;
mod cli;
mod crypt;
mod exefs;
mod keys;
mod le;
mod media;
mod ncch;
mod ncsd;
mod stream;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};

use crate::tools::Tool;
use crate::tools::three_ds_rom_tool::cia::{Certchain, Config};
pub use crate::tools::three_ds_rom_tool::cli::ThreeDsRomTool;
use crate::tools::three_ds_rom_tool::cli::{Command, TargetArgs};
use crate::tools::three_ds_rom_tool::crypt::{Direction, Target};
use crate::tools::three_ds_rom_tool::keys::KeySet;

impl Tool for ThreeDsRomTool {
    fn run(self) -> ExitCode {
        match run(self.command) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(command: Command) -> Result<ExitCode> {
    match command {
        Command::Decrypt { dev, target, roms } => {
            crypt(Direction::Decrypt, KeySet::from_dev(dev), &target, &roms)
        }
        Command::Encrypt { dev, target, roms } => {
            crypt(Direction::Encrypt, KeySet::from_dev(dev), &target, &roms)
        }
        Command::Cia {
            output,
            force,
            ignore_bad_hashes,
            ignore_encryption,
            dev,
            certchain_dev,
            roms,
        } => {
            let certchain = if dev {
                Certchain::dev(certchain_dev)?
            } else {
                Certchain::retail()
            };
            let config = Config {
                output,
                force,
                ignore_bad_hashes,
                ignore_encryption,
                keyset: KeySet::from_dev(dev),
                certchain,
            };
            Ok(each(&roms, |rom| cia::convert(rom, &config)))
        }
    }
}

fn crypt(
    direction: Direction,
    keyset: KeySet,
    args: &TargetArgs,
    roms: &[PathBuf],
) -> Result<ExitCode> {
    if args.output.is_some() && roms.len() > 1 {
        bail!("--output takes a single ROM, got {}", roms.len());
    }
    let target = if args.in_place {
        Target::InPlace
    } else {
        Target::Copy {
            output: args.output.as_deref(),
            force: args.force,
        }
    };
    Ok(each(roms, |rom| {
        crypt::run(rom, direction, keyset, &target)
    }))
}

fn each(roms: &[PathBuf], mut action: impl FnMut(&Path) -> Result<()>) -> ExitCode {
    let mut code = ExitCode::SUCCESS;
    for rom in roms {
        if let Err(error) = action(rom) {
            eprintln!("Error: {}: {error:?}", rom.display());
            code = ExitCode::FAILURE;
        }
    }
    code
}
