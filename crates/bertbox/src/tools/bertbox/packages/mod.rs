mod app;
mod download;
mod files;
mod flatpak;
mod github;
mod mac_app;
mod nightly;
mod package;
mod paths;
mod release;
mod sources;
mod termux;
mod version;

use std::fmt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::builder::{PossibleValuesParser, TypedValueParser};

use crate::install_target::{InstallTarget, System};
use crate::which;

pub use crate::tools::bertbox::packages::app::AppBundle;
pub use crate::tools::bertbox::packages::files::write as write_file;
pub use crate::tools::bertbox::packages::github::GitHubRepository;
pub use crate::tools::bertbox::packages::nightly::NightlyArtifact;
pub use crate::tools::bertbox::packages::package::Package;
pub use crate::tools::bertbox::packages::paths::{bin_dir, fish_dir, zsh_dir};
pub use crate::tools::bertbox::packages::release::{ChecksumAsset, Release, SlopAbstraction};
pub use crate::tools::bertbox::packages::version::Version;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackageName(&'static str);

impl PackageName {
    pub const fn new(name: &'static str) -> PackageName {
        PackageName(name)
    }

    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

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

pub enum Recipe {
    NightlyBinary(NightlyArtifact),
    NightlyApp(NightlyArtifact, AppBundle),
    ReleaseBinary(Release),
    TermuxPackage(GitHubRepository),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    Removed,
    NotInstalled,
}

pub struct Source {
    pub name: PackageName,
    pub recipe: fn(InstallTarget) -> Result<Recipe>,
    pub after_install: Option<(&'static str, &'static [&'static str])>,
}

impl Source {
    pub fn installed(&self, host: InstallTarget) -> Result<bool> {
        if host.0 == System::Termux {
            return Ok(which::first(self.name.as_str()).is_some());
        }
        if app::installed_target(self.name, host)?.is_some() {
            return Ok(true);
        }
        Ok(package::installed()?
            .iter()
            .any(|package| package.name() == self.name.as_str()))
    }

    pub fn selected_target(&self, host: InstallTarget) -> Result<InstallTarget> {
        Ok(app::installed_target(self.name, host)?.unwrap_or(host))
    }

    pub fn install(&self, target: InstallTarget, force: bool) -> Result<()> {
        let name = self.name;
        match (self.recipe)(target)? {
            Recipe::NightlyBinary(artifact) => {
                let package = Package::new(name.as_str())?;
                nightly::deploy(&package, target, &artifact, force)?;
                self.after_package_install(&package)
            }
            Recipe::ReleaseBinary(release) => {
                let package = Package::new(name.as_str())?;
                release::deploy(&package, name, target, &release, force)?;
                self.after_package_install(&package)
            }
            Recipe::NightlyApp(artifact, bundle) => app::deploy(name, &artifact, bundle, force),
            Recipe::TermuxPackage(repository) => {
                let binary = termux::deploy(name, repository)?;
                match self.after_install {
                    Some((_, args)) => run(&binary, args),
                    None => Ok(()),
                }
            }
        }
    }

    pub fn install_binary(&self, target: InstallTarget, binary: Vec<u8>) -> Result<()> {
        let name = self.name;
        match (self.recipe)(target)? {
            Recipe::NightlyBinary(_)
            | Recipe::ReleaseBinary(Release {
                executables: Executables::Named,
                ..
            }) => {
                let package = Package::new(name.as_str())?;
                package.replace_executable(vec![(PathBuf::from(name.as_str()), binary)])?;
                self.after_package_install(&package)
            }
            _ => bail!("installing a binary is not supported for {target}"),
        }
    }

    pub fn uninstall(&self, target: InstallTarget) -> Result<Removal> {
        match (self.recipe)(target)? {
            Recipe::NightlyBinary(_) | Recipe::ReleaseBinary(_) => {
                Package::new(self.name.as_str())?.remove()
            }
            Recipe::NightlyApp(..) | Recipe::TermuxPackage(_) => {
                bail!("{}: uninstall is not supported for {target}", self.name)
            }
        }
    }

    fn after_package_install(&self, package: &Package) -> Result<()> {
        match self.after_install {
            Some((program, args)) => package.run(program, args),
            None => Ok(()),
        }
    }
}

fn run(program: &std::path::Path, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("running {}", program.display()))?;
    if !status.success() {
        bail!("{} {} failed: {status}", program.display(), args.join(" "));
    }
    Ok(())
}

pub fn all() -> &'static [Source] {
    sources::ALL
}

pub fn names() -> Vec<&'static str> {
    all().iter().map(|source| source.name.as_str()).collect()
}

pub fn parser() -> impl TypedValueParser<Value = &'static Source> {
    PossibleValuesParser::new(names()).map(|name| {
        all()
            .iter()
            .find(|source| source.name.as_str() == name)
            .expect("the possible values are the source names")
    })
}
