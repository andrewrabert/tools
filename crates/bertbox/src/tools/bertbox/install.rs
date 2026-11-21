use std::process::ExitCode;

use anyhow::bail;
use clap::Args as ClapArgs;

use crate::install_target::{InstallTarget, PackageManager, System};
use crate::tools::Tool;
use crate::tools::bertbox::each::each_package;
use crate::tools::bertbox::packages::{self, Package};

#[derive(ClapArgs)]
#[command(about = "Install packages")]
pub struct Install {
    /// Package to install (repeatable)
    #[arg(value_name = "PACKAGE", required = true, value_parser = packages::parser())]
    packages: Vec<&'static Package>,

    /// Reinstall a package that is already up to date
    #[arg(short, long)]
    force: bool,

    /// Install through Flatpak. Linux only
    #[arg(long)]
    flatpak: bool,
}

impl Tool for Install {
    fn run(self) -> ExitCode {
        each_package(&self.packages, |package, host| {
            let InstallTarget(system, arch, _) = host;
            let package_manager = if self.flatpak {
                if system != System::Linux {
                    bail!("--flatpak is only supported on Linux, not {host}");
                }
                Some(PackageManager::Flatpak)
            } else {
                None
            };
            package.install(InstallTarget(system, arch, package_manager), self.force)
        })
    }
}
