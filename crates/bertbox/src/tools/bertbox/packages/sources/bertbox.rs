use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::System::{Linux, Macos, Termux};
use crate::tools::bertbox::packages::{
    GitHubRepository, NightlyArtifact, PackageName, Recipe, Source,
};

const REPOSITORY: GitHubRepository = GitHubRepository {
    owner: "andrewrabert",
    name: "tools",
};

pub const SOURCE: Source = Source {
    name: PackageName::new("bertbox"),
    recipe,
    after_install: Some(("bertbox", &["link"])),
};

fn recipe(target: InstallTarget) -> Result<Recipe> {
    let artifact = match target {
        InstallTarget(Linux, X86_64, None) => "bertbox-x86_64-unknown-linux-musl.zip",
        InstallTarget(Linux, Aarch64, None) => "bertbox-aarch64-unknown-linux-musl.zip",
        InstallTarget(Macos, X86_64, None) => "bertbox-x86_64-apple-darwin.zip",
        InstallTarget(Macos, Aarch64, None) => "bertbox-aarch64-apple-darwin.zip",
        InstallTarget(Termux, Aarch64, None) => {
            return Ok(Recipe::TermuxPackage(REPOSITORY));
        }
        _ => bail!("unsupported platform: {target}"),
    };
    Ok(Recipe::NightlyBinary(NightlyArtifact {
        repository: REPOSITORY,
        workflow: "ci",
        artifact,
    }))
}
