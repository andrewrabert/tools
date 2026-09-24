mod cli;
mod links;
mod roots;
mod sources;

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result};
use nix::unistd;

use crate::tools::Tool;
pub use crate::tools::dotfiles_link_bin::cli::DotfilesLinkBin;
use crate::tools::dotfiles_link_bin::links::Links;
use crate::tools::dotfiles_link_bin::roots::Roots;

const HOST_VARIABLE: &str = "HOST_DOTFILES";
const BIN_DIR: &str = ".local/bin";
const LEGACY_BIN_DIR: &str = ".bin";

impl Tool for DotfilesLinkBin {
    fn run(self) -> ExitCode {
        match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run() -> Result<()> {
    let roots = Roots::from_env()?;
    let host = host()?;
    let dest = roots.primary.join(BIN_DIR);
    fs::create_dir_all(&dest).with_context(|| format!("creating {}", dest.display()))?;

    let sources = sources::collect(&roots, &host, &dest)?;
    Links::from_sources(&sources)?.install(&dest)?;
    link_legacy_bin(&roots.primary.join(LEGACY_BIN_DIR), &dest)
}

fn host() -> Result<OsString> {
    match env::var_os(HOST_VARIABLE) {
        Some(name) if !name.is_empty() => Ok(name),
        _ => unistd::gethostname().context("reading the hostname"),
    }
}

fn link_legacy_bin(legacy: &Path, dest: &Path) -> Result<()> {
    if legacy.is_dir() && !legacy.is_symlink() {
        fs::remove_dir_all(legacy).with_context(|| format!("removing {}", legacy.display()))?;
    }
    links::point(legacy, dest)
}
