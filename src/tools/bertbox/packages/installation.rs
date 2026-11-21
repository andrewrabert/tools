use std::fs;
use std::io::ErrorKind;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::install_target::InstallTarget;
use crate::tools::bertbox::packages::{PackageName, files, paths};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Version {
    Etag(String),
    Tag(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installation {
    pub target: InstallTarget,
    pub version: Version,
    pub sha256: Option<String>,
}

impl Installation {
    pub fn load(name: PackageName) -> Result<Option<Installation>> {
        let path = paths::installation_file(name)?;
        match fs::read_to_string(&path) {
            Ok(text) => Ok(serde_json::from_str(&text).ok()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn store(&self, name: PackageName) -> Result<()> {
        let rendered = serde_json::to_string_pretty(self).context("rendering the installation")?;
        files::write(
            &paths::installation_file(name)?,
            format!("{rendered}\n").as_bytes(),
        )
    }
}
