use std::path::Path;

use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::System::{Linux, Macos, Termux};
use crate::tools::bertbox::link;
use crate::tools::bertbox::packages::{Install, Package, paths};

const REPOSITORY: &str = "andrewrabert/tools";

pub const PACKAGE: Package = Package {
    name: "bertbox",
    install,
    after_install: Some(link_tools),
};

fn install(target: InstallTarget) -> Result<Install> {
    let artifact = match target {
        InstallTarget(Linux, X86_64, None) => "bertbox-x86_64-unknown-linux-musl.zip",
        InstallTarget(Linux, Aarch64, None) => "bertbox-aarch64-unknown-linux-musl.zip",
        InstallTarget(Macos, X86_64, None) => "bertbox-x86_64-apple-darwin.zip",
        InstallTarget(Macos, Aarch64, None) => "bertbox-aarch64-apple-darwin.zip",
        InstallTarget(Termux, Aarch64, None) => {
            return Ok(Install::Termux {
                repository: REPOSITORY,
            });
        }
        _ => bail!("unsupported platform: {target}"),
    };
    Ok(Install::NightlyBinary {
        repository: REPOSITORY,
        artifact,
    })
}

fn link_tools(binary: &Path) -> Result<()> {
    link::link_tools(&paths::bin_dir()?, binary)
}
