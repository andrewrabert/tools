mod app;
mod catalog;
mod download;
mod files;
mod flatpak;
mod github;
mod installation;
mod mac_app;
mod nightly;
mod paths;
mod release;
mod termux;

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::builder::{PossibleValuesParser, TypedValueParser};

use crate::install_target::{InstallTarget, System};
use crate::which;

pub use crate::tools::bertbox::packages::app::AppBundle;
pub use crate::tools::bertbox::packages::files::write as write_file;
pub use crate::tools::bertbox::packages::github::GitHubRepository;
pub use crate::tools::bertbox::packages::installation::{Installation, Version};
pub use crate::tools::bertbox::packages::nightly::NightlyArtifact;
pub use crate::tools::bertbox::packages::paths::{bin_dir, fish_dir};
pub use crate::tools::bertbox::packages::release::{ChecksumAsset, Release};

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

pub struct Package {
    pub name: PackageName,
    pub recipe: fn(InstallTarget) -> Result<Recipe>,
    pub after_install: Option<fn(&Path) -> Result<()>>,
}

impl Package {
    pub fn installed(&self, host: InstallTarget) -> Result<bool> {
        if host.0 == System::Termux {
            return Ok(which::first(self.name.as_str()).is_some());
        }
        Ok(paths::package_dir(self.name)?.exists())
    }

    pub fn selected_target(&self, host: InstallTarget) -> Result<InstallTarget> {
        Ok(self
            .installation()?
            .map_or(host, |installation| installation.target))
    }

    pub fn install(&self, target: InstallTarget, force: bool) -> Result<()> {
        let name = self.name;
        let binary: Option<PathBuf> = match (self.recipe)(target)? {
            Recipe::NightlyBinary(artifact) => {
                Some(nightly::deploy(name, target, &artifact, force)?)
            }
            Recipe::NightlyApp(artifact, bundle) => {
                app::deploy(name, target, &artifact, bundle, force)?;
                None
            }
            Recipe::ReleaseBinary(release) => {
                release::deploy(name, target, &release, force)?;
                None
            }
            Recipe::TermuxPackage(repository) => Some(termux::deploy(name, repository)?),
        };
        match (self.after_install, binary) {
            (Some(after_install), Some(binary)) => after_install(&binary),
            _ => Ok(()),
        }
    }

    pub fn uninstall(&self, target: InstallTarget) -> Result<Removal> {
        match (self.recipe)(target)? {
            Recipe::NightlyBinary(_) | Recipe::ReleaseBinary(_) => files::remove_package(self.name),
            Recipe::NightlyApp(..) | Recipe::TermuxPackage(_) => {
                bail!("{}: uninstall is not supported for {target}", self.name)
            }
        }
    }

    pub fn installation(&self) -> Result<Option<Installation>> {
        Installation::load(self.name)
    }
}

pub fn all() -> &'static [Package] {
    catalog::ALL
}

pub fn names() -> Vec<&'static str> {
    all().iter().map(|package| package.name.as_str()).collect()
}

pub fn parser() -> impl TypedValueParser<Value = &'static Package> {
    PossibleValuesParser::new(names()).map(|name| {
        all()
            .iter()
            .find(|package| package.name.as_str() == name)
            .expect("the possible values are the package names")
    })
}
