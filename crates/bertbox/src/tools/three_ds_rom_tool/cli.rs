//! clap argument definitions.
use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand};

#[derive(ClapArgs)]
#[command(about = "Decrypt, encrypt, or convert 3DS (NCSD/CCI) ROMs")]
pub struct ThreeDsRomTool {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(
        about = "decrypt .3ds files (default: write <name>.decrypted.<ext> next to each ROM)"
    )]
    Decrypt {
        #[arg(long, help = "use development keys")]
        dev: bool,
        #[command(flatten)]
        target: TargetArgs,
        #[arg(required = true, value_name = "ROM")]
        roms: Vec<PathBuf>,
    },
    #[command(
        about = "encrypt .3ds files (default: write <name>.encrypted.<ext> next to each ROM)"
    )]
    Encrypt {
        #[arg(long, help = "use development keys")]
        dev: bool,
        #[command(flatten)]
        target: TargetArgs,
        #[arg(required = true, value_name = "ROM")]
        roms: Vec<PathBuf>,
    },
    #[command(about = "convert .3ds/.cci files to .cia (keeps the ROM's encryption)")]
    Cia {
        #[arg(
            short,
            long,
            value_name = "DIR",
            default_value = ".",
            help = "output directory"
        )]
        output: PathBuf,
        #[arg(short, long, help = "overwrite existing .cia files")]
        force: bool,
        #[arg(long, help = "convert even if the ExtHeader hash is invalid")]
        ignore_bad_hashes: bool,
        #[arg(long, help = "assume the ROM is decrypted and set the NoCrypto flag")]
        ignore_encryption: bool,
        #[arg(long, help = "use development keys and the dev certificate chain")]
        dev: bool,
        #[arg(
            long,
            value_name = "FILE",
            requires = "dev",
            help = "dev certificate chain (default: ./certchain-dev.bin or ~/.3ds/certchain-dev.bin)"
        )]
        certchain_dev: Option<PathBuf>,
        #[arg(required = true, value_name = "ROM")]
        roms: Vec<PathBuf>,
    },
}

#[derive(ClapArgs)]
pub struct TargetArgs {
    #[arg(
        short,
        long,
        value_name = "FILE",
        help = "output file (single ROM only)"
    )]
    pub output: Option<PathBuf>,
    #[arg(short, long, help = "overwrite an existing output file")]
    pub force: bool,
}
