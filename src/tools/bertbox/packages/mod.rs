mod app;
mod catalog;
mod download;
mod files;
mod flatpak;
mod mac_app;
mod nightly;
mod paths;
mod record;
mod release;
mod termux;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, anyhow, bail};
use serde_json::Value;

use crate::install_target::{InstallTarget, System};
use crate::which;

pub use crate::tools::bertbox::packages::files::write as write_file;
pub use crate::tools::bertbox::packages::paths::{bin_dir, zsh_dir};

#[derive(Clone, Copy)]
pub enum Archive {
    TarGz,
    TarXz,
}

#[derive(Clone, Copy)]
pub enum Executables {
    Named,
    All,
}

pub enum Install {
    NightlyBinary {
        repository: &'static str,
        artifact: &'static str,
    },
    ReleaseBinary {
        owner: &'static str,
        repository: &'static str,
        asset: &'static str,
        archive: Archive,
        checksums: Option<fn(tag: &str) -> String>,
        executables: Executables,
    },
    NightlyFlatpak {
        repository: &'static str,
        workflow: &'static str,
        artifact: &'static str,
    },
    NightlyMacApp {
        repository: &'static str,
        workflow: &'static str,
        artifact: &'static str,
        app: &'static str,
    },
    Termux {
        repository: &'static str,
    },
}

pub struct Package {
    pub name: &'static str,
    pub install: fn(InstallTarget) -> Result<Install>,
    pub after_install: Option<fn(&Path) -> Result<()>>,
}

impl Package {
    pub fn installed(&self, host: InstallTarget) -> Result<bool> {
        if host.0 == System::Termux {
            return Ok(which::first(self.name).is_some());
        }
        Ok(paths::package_dir(self.name)?.exists())
    }

    pub fn selected_target(&self, host: InstallTarget) -> Result<InstallTarget> {
        Ok(record::target(self.name)?.unwrap_or(host))
    }

    pub fn install(&self, target: InstallTarget, force: bool) -> Result<()> {
        let name = self.name;
        let binary: Option<PathBuf> = match (self.install)(target)? {
            Install::NightlyBinary {
                repository,
                artifact,
            } => Some(nightly::install(name, target, repository, artifact, force)?),
            Install::ReleaseBinary {
                owner,
                repository,
                asset,
                archive,
                checksums,
                executables,
            } => {
                let release = release::Release {
                    owner,
                    repository,
                    asset,
                    archive,
                    checksums,
                    executables,
                };
                release::install(name, target, &release, force)?;
                None
            }
            Install::NightlyFlatpak {
                repository,
                workflow,
                artifact,
            } => {
                let source = app::Source {
                    repository,
                    workflow,
                    artifact,
                };
                app::install(name, target, &source, app::Installer::Flatpak, force)?;
                None
            }
            Install::NightlyMacApp {
                repository,
                workflow,
                artifact,
                app,
            } => {
                let source = app::Source {
                    repository,
                    workflow,
                    artifact,
                };
                app::install(name, target, &source, app::Installer::MacApp { app }, force)?;
                None
            }
            Install::Termux { repository } => Some(termux::install(name, repository)?),
        };
        match (self.after_install, binary) {
            (Some(after_install), Some(binary)) => after_install(&binary),
            _ => Ok(()),
        }
    }

    pub fn uninstall(&self, target: InstallTarget) -> Result<bool> {
        match (self.install)(target)? {
            Install::NightlyBinary { .. } | Install::ReleaseBinary { .. } => {
                files::remove_package(self.name)
            }
            Install::NightlyFlatpak { .. }
            | Install::NightlyMacApp { .. }
            | Install::Termux { .. } => {
                bail!("{}: uninstall is not supported for {target}", self.name)
            }
        }
    }

    pub fn record(&self) -> Result<Option<Value>> {
        record::read_value(self.name)
    }
}

pub fn all() -> &'static [Package] {
    catalog::ALL
}

pub fn names() -> Vec<&'static str> {
    all().iter().map(|package| package.name).collect()
}

pub fn run_each(
    names: &[String],
    action: impl Fn(&Package, InstallTarget) -> Result<()>,
) -> ExitCode {
    let host = match InstallTarget::host() {
        Ok(host) => host,
        Err(error) => {
            eprintln!("Error: {error:?}");
            return ExitCode::FAILURE;
        }
    };
    let mut status = ExitCode::SUCCESS;
    for name in names {
        let result = match all().iter().find(|package| package.name == name) {
            Some(package) => action(package, host),
            None => Err(anyhow!("unknown package {name}")),
        };
        if let Err(error) = result {
            eprintln!("Error: {name}: {error:?}");
            status = ExitCode::FAILURE;
        }
    }
    status
}
