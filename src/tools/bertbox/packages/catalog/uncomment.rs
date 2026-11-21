use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::System::{Linux, Macos};
use crate::tools::bertbox::packages::{Archive, Executables, Install, Package};

pub const PACKAGE: Package = Package {
    name: "uncomment",
    install,
    after_install: None,
};

fn install(target: InstallTarget) -> Result<Install> {
    let asset = match target {
        InstallTarget(Linux, X86_64, None) => "uncomment-x86_64-unknown-linux-gnu.tar.gz",
        InstallTarget(Linux, Aarch64, None) => "uncomment-aarch64-unknown-linux-gnu.tar.gz",
        InstallTarget(Macos, X86_64, None) => "uncomment-x86_64-apple-darwin.tar.gz",
        InstallTarget(Macos, Aarch64, None) => "uncomment-aarch64-apple-darwin.tar.gz",
        _ => bail!("unsupported platform: {target}"),
    };
    Ok(Install::ReleaseBinary {
        owner: "polylint",
        repository: "uncomment",
        asset,
        archive: Archive::TarGz,
        checksums: Some(checksums),
        executables: Executables::Named,
    })
}

fn checksums(tag: &str) -> String {
    let version = tag.strip_prefix('v').unwrap_or(tag);
    format!("uncomment_{version}_checksums.txt")
}
