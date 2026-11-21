use std::path::PathBuf;

use anyhow::Result;

use crate::tools::bertbox::packages::PackageName;
use crate::{APP_NAME, dirs};

fn data_root() -> Result<PathBuf> {
    Ok(dirs::data()?.join(APP_NAME))
}

pub fn bin_dir() -> Result<PathBuf> {
    Ok(data_root()?.join("bin"))
}

pub fn zsh_dir() -> Result<PathBuf> {
    Ok(data_root()?.join("zsh"))
}

pub fn package_dir(name: PackageName) -> Result<PathBuf> {
    Ok(data_root()?.join("pkgs").join(name.as_str()))
}

pub fn package_bin(name: PackageName) -> Result<PathBuf> {
    Ok(package_dir(name)?.join("bin"))
}

pub fn binary(name: PackageName) -> Result<PathBuf> {
    Ok(package_bin(name)?.join(name.as_str()))
}

pub fn installation_file(name: PackageName) -> Result<PathBuf> {
    Ok(package_dir(name)?.join("version.json"))
}
