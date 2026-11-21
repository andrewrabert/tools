use std::path::PathBuf;

use anyhow::Result;

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

pub fn package_dir(name: &str) -> Result<PathBuf> {
    Ok(data_root()?.join("pkgs").join(name))
}

pub fn package_bin(name: &str) -> Result<PathBuf> {
    Ok(package_dir(name)?.join("bin"))
}

pub fn binary(name: &str) -> Result<PathBuf> {
    Ok(package_bin(name)?.join(name))
}

pub fn record(name: &str) -> Result<PathBuf> {
    Ok(package_dir(name)?.join("version.json"))
}
