use std::path::PathBuf;

use anyhow::Result;

use crate::{APP_NAME, dirs};

fn data_root() -> Result<PathBuf> {
    Ok(dirs::data()?.join(APP_NAME))
}

pub fn bin_dir() -> Result<PathBuf> {
    Ok(data_root()?.join("bin"))
}

pub fn fish_dir() -> Result<PathBuf> {
    Ok(data_root()?.join("fish"))
}

pub fn pkgs_dir() -> Result<PathBuf> {
    Ok(data_root()?.join("pkgs"))
}

pub fn system_dir() -> Result<PathBuf> {
    Ok(data_root()?.join("system"))
}
