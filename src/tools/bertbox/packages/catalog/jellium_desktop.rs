use anyhow::{Result, bail};

use crate::install_target::Arch::{Aarch64, X86_64};
use crate::install_target::InstallTarget;
use crate::install_target::PackageManager::Flatpak;
use crate::install_target::System::{Linux, Macos};
use crate::tools::bertbox::packages::{Install, Package};

const REPOSITORY: &str = "andrewrabert/jellium-desktop";

pub const PACKAGE: Package = Package {
    name: "jellium-desktop",
    install,
    after_install: None,
};

fn install(target: InstallTarget) -> Result<Install> {
    match target {
        InstallTarget(Linux, X86_64, Some(Flatpak)) => Ok(Install::NightlyFlatpak {
            repository: REPOSITORY,
            workflow: "build-linux-flatpak",
            artifact: "linux-flatpak-x86_64.zip",
        }),
        InstallTarget(Macos, Aarch64, None) => Ok(Install::NightlyMacApp {
            repository: REPOSITORY,
            workflow: "build-macos",
            artifact: "macos-arm64.zip",
            app: "Jellium Desktop.app",
        }),
        _ => bail!("unsupported platform: {target}"),
    }
}
