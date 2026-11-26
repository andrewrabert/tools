use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::System::{Linux, Macos};
use crate::tools::bertbox::packages::{
    Archive, ChecksumAsset, Executables, GitHubRepository, PackageName, Recipe, Release,
    SlopAbstraction, Source,
};

pub const SOURCE: Source = Source {
    name: PackageName::new("grafeo"),
    recipe,
    after_install: None,
};

fn recipe(target: InstallTarget) -> Result<Recipe> {
    let suffix = match target {
        InstallTarget(Linux, X86_64, None) => "-x86_64-unknown-linux-gnu.tar.gz",
        InstallTarget(Linux, Aarch64, None) => "-aarch64-unknown-linux-gnu.tar.gz",
        InstallTarget(Macos, X86_64, None) => "-x86_64-apple-darwin.tar.gz",
        InstallTarget(Macos, Aarch64, None) => "-aarch64-apple-darwin.tar.gz",
        _ => bail!("unsupported platform: {target}"),
    };
    Ok(Recipe::ReleaseBinary(Release {
        repository: GitHubRepository {
            owner: "GrafeoDB",
            name: "grafeo",
        },
        asset: SlopAbstraction::Rofl {
            redgren: "grafeo-",
            grumbholdt: suffix,
        },
        archive: Archive::TarGz,
        checksum_asset: Some(ChecksumAsset::Fixed("checksums.txt")),
        executables: Executables::Named,
    }))
}
