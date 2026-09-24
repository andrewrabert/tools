use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};

const PRIMARY: &str = "DOTFILES";
const EXTRA_PREFIX: &str = "DOTFILES_";

pub struct Roots {
    pub primary: PathBuf,
    pub extras: Vec<PathBuf>,
}

impl Roots {
    pub fn from_env() -> Result<Self> {
        let primary = env::var_os(PRIMARY).with_context(|| format!("{PRIMARY} is not set"))?;
        let mut extras: Vec<_> = env::vars_os()
            .filter(|(name, _)| name.to_string_lossy().starts_with(EXTRA_PREFIX))
            .collect();
        extras.sort();
        Ok(Roots {
            primary: primary.into(),
            extras: extras
                .into_iter()
                .map(|(_, value)| PathBuf::from(value))
                .collect(),
        })
    }
}
