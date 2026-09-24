//! The user's base directories. The only module that talks to `etcetera`.
use std::path::PathBuf;

use anyhow::{Context, Result};
use etcetera::BaseStrategy;
use etcetera::base_strategy::Xdg;

fn xdg() -> Result<Xdg> {
    Xdg::new().context("locating the home directory")
}

pub fn home() -> Result<PathBuf> {
    etcetera::home_dir().context("locating the home directory")
}

pub fn cache() -> Result<PathBuf> {
    Ok(xdg()?.cache_dir())
}

pub fn config() -> Result<PathBuf> {
    Ok(xdg()?.config_dir())
}

pub fn runtime() -> Result<PathBuf> {
    xdg()?.runtime_dir().context("XDG_RUNTIME_DIR is not set")
}
