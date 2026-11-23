use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::System::{Linux, Macos};
use crate::tools::bertbox::packages::{
    Archive, Executables, GitHubRepository, PackageName, Recipe, Release, Source,
};

pub const SOURCE: Source = Source {
    name: PackageName::new("aweber-cli"),
    recipe,
    after_install: None,
};

fn recipe(target: InstallTarget) -> Result<Recipe> {
    let asset = match target {
        InstallTarget(Linux, X86_64, None) => "aweber-x86_64-unknown-linux-musl.tar.gz",
        InstallTarget(Linux, Aarch64, None) => "aweber-aarch64-unknown-linux-musl.tar.gz",
        InstallTarget(Macos, X86_64, None) => "aweber-x86_64-apple-darwin.tar.gz",
        InstallTarget(Macos, Aarch64, None) => "aweber-aarch64-apple-darwin.tar.gz",
        _ => bail!("unsupported platform: {target}"),
    };
    Ok(Recipe::ReleaseBinary(Release {
        repository: GitHubRepository {
            owner: "andrewrabert",
            name: "aweber-cli",
        },
        asset,
        archive: Archive::TarGz,
        checksum_asset: None,
        executables: Executables::All,
    }))
}
