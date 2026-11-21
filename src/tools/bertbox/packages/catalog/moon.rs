use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::System::{Linux, Macos};
use crate::tools::bertbox::packages::{Archive, Executables, Install, Package};

pub const PACKAGE: Package = Package {
    name: "moon",
    install,
    after_install: None,
};

fn install(target: InstallTarget) -> Result<Install> {
    let asset = match target {
        InstallTarget(Linux, X86_64, None) => "moon_cli-x86_64-unknown-linux-gnu.tar.xz",
        InstallTarget(Linux, Aarch64, None) => "moon_cli-aarch64-unknown-linux-gnu.tar.xz",
        InstallTarget(Macos, X86_64, None) => "moon_cli-x86_64-apple-darwin.tar.xz",
        InstallTarget(Macos, Aarch64, None) => "moon_cli-aarch64-apple-darwin.tar.xz",
        _ => bail!("unsupported platform: {target}"),
    };
    Ok(Install::ReleaseBinary {
        owner: "moonrepo",
        repository: "moon",
        asset,
        archive: Archive::TarXz,
        checksums: Some(checksums),
        executables: Executables::Named,
    })
}

fn checksums(_tag: &str) -> String {
    "sha256.sum".to_owned()
}
