use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::System::{Linux, Macos};
use crate::tools::bertbox::packages::{
    Archive, Executables, GitHubRepository, PackageName, Recipe, Release, Source,
};

pub const SOURCE: Source = Source {
    name: PackageName::new("zoekt-simple"),
    recipe,
    after_install: None,
};

fn recipe(target: InstallTarget) -> Result<Recipe> {
    let asset = match target {
        InstallTarget(Linux, X86_64, None) => "zoekt-linux-amd64.tar.gz",
        InstallTarget(Linux, Aarch64, None) => "zoekt-linux-arm64.tar.gz",
        InstallTarget(Macos, Aarch64, None) => "zoekt-darwin-arm64.tar.gz",
        _ => bail!("unsupported platform: {target}"),
    };
    Ok(Recipe::ReleaseBinary(Release {
        repository: GitHubRepository {
            owner: "andrewrabert",
            name: "zoekt-simple",
        },
        asset,
        archive: Archive::TarGz,
        checksum_asset: None,
        executables: Executables::All,
    }))
}
