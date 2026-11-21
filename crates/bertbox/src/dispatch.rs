use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};

pub const ENV_NAME: &str = "BERTBOX_AS_BERTBOX";
pub const ENV_VALUE: &str = "1";

pub fn program() -> Result<PathBuf> {
    env::current_exe().context("locating own executable")
}
