use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand};

#[derive(ClapArgs)]
#[command(about = "Manage trash and libraries on an Immich server")]
pub struct ImmichRemote {
    #[arg(long, env = "IMMICH_URL", help = "base url of the server")]
    pub url: String,
    #[arg(long, env = "IMMICH_API_KEY", hide_env_values = true, help = "api key")]
    pub api_key: String,
    #[arg(
        long = "parent",
        global = true,
        value_name = "DIR",
        help = "directory whose trashed files may be touched (repeatable)"
    )]
    pub parents: Vec<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "empty trash")]
    EmptyTrash {
        #[arg(long)]
        dryrun: bool,
        #[arg(long, help = "only delete assets whose file is missing")]
        missing_only: bool,
    },
    #[command(about = "restore trash")]
    RestoreTrash {
        #[arg(long)]
        dryrun: bool,
    },
    #[command(about = "scan libraries")]
    Scan,
}
