use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::System::{Linux, Macos};
use crate::tools::bertbox::packages::{
    GitHubRepository, NightlyArtifact, PackageName, Recipe, Source,
};

pub const SOURCE: Source = Source {
    name: PackageName::new("jevrev"),
    recipe,
    after_install: None,
};

fn recipe(target: InstallTarget) -> Result<Recipe> {
    let artifact = match target {
        InstallTarget(Linux, X86_64, None) => "jevrev-x86_64-unknown-linux-musl.zip",
        InstallTarget(Linux, Aarch64, None) => "jevrev-aarch64-unknown-linux-musl.zip",
        InstallTarget(Macos, X86_64, None) => "jevrev-x86_64-apple-darwin.zip",
        InstallTarget(Macos, Aarch64, None) => "jevrev-aarch64-apple-darwin.zip",
        _ => bail!("unsupported platform: {target}"),
    };
    Ok(Recipe::NightlyBinary(NightlyArtifact {
        repository: GitHubRepository {
            owner: "andrewrabert",
            name: "jevrev",
        },
        workflow: "ci",
        artifact,
    }))
}
