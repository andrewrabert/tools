//! clap argument definitions.
use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand};

use crate::tools::cdemu_tool::device::SrDevice;

#[derive(ClapArgs)]
#[command(about = "Load disc images into CDEmu daemons run as systemd user units")]
pub struct CdemuTool {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "load images into a new device")]
    Load {
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
    },
    #[command(about = "remove a device")]
    Remove {
        #[arg(
            value_name = "DEVICE",
            help = "sr device path, as shown by status",
            value_parser = clap::value_parser!(SrDevice)
        )]
        device: SrDevice,
    },
    #[command(about = "show devices")]
    Status {
        #[arg(long, help = "output as JSON")]
        json: bool,
    },
    #[command(about = "run the daemon inside the unit started by load")]
    SystemdLoad {
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
    },
}
