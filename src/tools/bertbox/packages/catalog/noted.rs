use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::System::{Linux, Macos, Termux};
use crate::tools::bertbox::packages::{Install, Package};

const REPOSITORY: &str = "andrewrabert/noted";

pub const PACKAGE: Package = Package {
    name: "noted",
    install,
    after_install: None,
};

fn install(target: InstallTarget) -> Result<Install> {
    let artifact = match target {
        InstallTarget(Linux, X86_64, None) => "noted-x86_64-unknown-linux-musl.zip",
        InstallTarget(Linux, Aarch64, None) => "noted-aarch64-unknown-linux-musl.zip",
        InstallTarget(Macos, X86_64, None) => "noted-x86_64-apple-darwin.zip",
        InstallTarget(Macos, Aarch64, None) => "noted-aarch64-apple-darwin.zip",
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
