use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};

const PRIMARY: &str = "DOTFILES";
const EXTRA_PREFIX: &str = "DOTFILES_";
const SCRIPT_DIR: &str = "full-update";

pub struct Roots(Vec<PathBuf>);

impl Roots {
    pub fn from_env() -> Result<Self> {
        let primary = env::var_os(PRIMARY).with_context(|| format!("{PRIMARY} is not set"))?;
        let mut extras: Vec<_> = env::vars_os()
            .filter(|(name, _)| name.to_string_lossy().starts_with(EXTRA_PREFIX))
            .collect();
        extras.sort();
        let roots = std::iter::once(primary)
            .chain(extras.into_iter().map(|(_, value)| value))
            .map(PathBuf::from)
            .collect();
        Ok(Roots(roots))
    }

    pub fn script_dirs(&self) -> impl Iterator<Item = PathBuf> {
        self.0.iter().map(|root| root.join(SCRIPT_DIR))
    }
}
