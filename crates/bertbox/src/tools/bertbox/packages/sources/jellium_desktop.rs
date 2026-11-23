use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::PackageManager::Flatpak;
use crate::install_target::System::{Linux, Macos};
use crate::tools::bertbox::packages::{
    AppBundle, GitHubRepository, NightlyArtifact, PackageName, Recipe, Source,
};

const REPOSITORY: GitHubRepository = GitHubRepository {
    owner: "andrewrabert",
    name: "jellium-desktop",
};

pub const SOURCE: Source = Source {
    name: PackageName::new("jellium-desktop"),
    recipe,
    after_install: None,
};

fn recipe(target: InstallTarget) -> Result<Recipe> {
    match target {
        InstallTarget(Linux, X86_64, Some(Flatpak)) => Ok(Recipe::NightlyApp(
            NightlyArtifact {
                repository: REPOSITORY,
                workflow: "build-linux-flatpak",
                artifact: "linux-flatpak-x86_64.zip",
            },
            AppBundle::Flatpak,
        )),
        InstallTarget(Macos, Aarch64, None) => Ok(Recipe::NightlyApp(
            NightlyArtifact {
                repository: REPOSITORY,
                workflow: "build-macos",
                artifact: "macos-arm64.zip",
            },
            AppBundle::MacApp {
                app: "Jellium Desktop.app",
            },
        )),
        _ => bail!("unsupported platform: {target}"),
    }
}
