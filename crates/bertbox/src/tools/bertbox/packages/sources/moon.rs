use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::System::{Linux, Macos};
use crate::tools::bertbox::packages::{
    Archive, ChecksumAsset, Executables, GitHubRepository, PackageName, Recipe, Release, Source,
};

pub const SOURCE: Source = Source {
    name: PackageName::new("moon"),
    recipe,
    after_install: None,
};

fn recipe(target: InstallTarget) -> Result<Recipe> {
    let asset = match target {
        InstallTarget(Linux, X86_64, None) => "moon_cli-x86_64-unknown-linux-gnu.tar.xz",
        InstallTarget(Linux, Aarch64, None) => "moon_cli-aarch64-unknown-linux-gnu.tar.xz",
        InstallTarget(Macos, X86_64, None) => "moon_cli-x86_64-apple-darwin.tar.xz",
        InstallTarget(Macos, Aarch64, None) => "moon_cli-aarch64-apple-darwin.tar.xz",
        _ => bail!("unsupported platform: {target}"),
    };
    Ok(Recipe::ReleaseBinary(Release {
        repository: GitHubRepository {
            owner: "moonrepo",
            name: "moon",
        },
        asset,
        archive: Archive::TarXz,
        checksum_asset: Some(ChecksumAsset::Fixed("sha256.sum")),
        executables: Executables::Named,
    }))
}
