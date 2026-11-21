use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::System::{Linux, Macos};
use crate::tools::bertbox::packages::{Archive, Executables, Install, Package};

pub const PACKAGE: Package = Package {
    name: "zoekt-simple",
    install,
    after_install: None,
};

fn install(target: InstallTarget) -> Result<Install> {
    let asset = match target {
        InstallTarget(Linux, X86_64, None) => "zoekt-linux-amd64.tar.gz",
        InstallTarget(Linux, Aarch64, None) => "zoekt-linux-arm64.tar.gz",
        InstallTarget(Macos, Aarch64, None) => "zoekt-darwin-arm64.tar.gz",
        _ => bail!("unsupported platform: {target}"),
    };
    Ok(Install::ReleaseBinary {
        owner: "andrewrabert",
        repository: "zoekt-simple",
        asset,
        archive: Archive::TarGz,
        checksums: None,
        executables: Executables::All,
    })
}
