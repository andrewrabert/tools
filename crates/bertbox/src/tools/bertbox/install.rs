use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, bail};
use clap::Args as ClapArgs;

use crate::install_target::{InstallTarget, PackageManager, System};
use crate::tools::Tool;
use crate::tools::bertbox::each::each_package;
use crate::tools::bertbox::packages::{self, Source};

#[derive(ClapArgs)]
#[command(about = "Install packages")]
pub struct Install {
    /// Package to install (repeatable)
    #[arg(value_name = "PACKAGE", required = true, value_parser = packages::parser())]
    packages: Vec<&'static Source>,

    /// Reinstall a package that is already up to date
    #[arg(short, long)]
    force: bool,

    /// Install through Flatpak. Linux only
    #[arg(long)]
    flatpak: bool,

    /// Install this binary instead of downloading
    #[arg(short, long, value_name = "PATH", conflicts_with_all = ["force", "flatpak"])]
    binary: Option<PathBuf>,
}

impl Tool for Install {
    fn run(self) -> ExitCode {
        if let Some(binary) = &self.binary {
            if self.packages.len() != 1 {
                eprintln!("Error: --binary takes exactly one package");
                return ExitCode::FAILURE;
            }
            return each_package(&self.packages, |source, host| {
                let data =
                    fs::read(binary).with_context(|| format!("reading {}", binary.display()))?;
                source.install_binary(host, data)
            });
        }
        each_package(&self.packages, |source, host| {
            let InstallTarget(system, arch, _) = host;
            let package_manager = if self.flatpak {
                if system != System::Linux {
                    bail!("--flatpak is only supported on Linux, not {host}");
                }
                Some(PackageManager::Flatpak)
            } else {
                None
            };
            source.install(InstallTarget(system, arch, package_manager), self.force)
        })
    }
}
