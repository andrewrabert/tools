use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::{Args as ClapArgs, Subcommand};
use nix::unistd::gethostname;

use crate::tools::pi_config::spec::HostName;

#[derive(ClapArgs)]
#[command(about = "Manage Pi settings, models and packages from a spec directory")]
pub struct PiConfig {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "install packages and apply a spec directory")]
    Apply {
        #[arg(value_name = "DIR")]
        dir: PathBuf,
        #[command(flatten)]
        host: Host,
    },
    #[command(about = "write the current settings and models as a spec to a directory")]
    Export {
        #[arg(value_name = "DIR")]
        dir: PathBuf,
        #[arg(short, long, help = "overwrite a spec that already exists")]
        force: bool,
    },
    #[command(about = "validate a spec directory without changing anything")]
    Validate {
        #[arg(value_name = "DIR")]
        dir: PathBuf,
        #[command(flatten)]
        host: Host,
    },
}

#[derive(ClapArgs)]
pub struct Host {
    #[arg(
        long,
        env = "HOST_DOTFILES",
        value_name = "HOST",
        help = "host whose hosts/HOST.yaml is merged in; this machine's hostname when omitted"
    )]
    host: Option<String>,
}

impl Host {
    pub fn resolve(self) -> Result<HostName> {
        let name = match self.host {
            Some(host) if !host.is_empty() => host,
            _ => gethostname()?
                .into_string()
                .map_err(|name| anyhow!("hostname is not UTF-8: {name:?}"))?,
        };
        HostName::new(name)
    }
}
