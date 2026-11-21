use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::tools::archive::tree;

pub struct Members {
    pub cwd: PathBuf,
    pub relative: Vec<PathBuf>,
}

impl Members {
    pub fn of(source: &Path) -> Result<Self> {
        if source.is_dir() {
            let mut relative = Vec::new();
            for path in tree::paths(source)? {
                if path != source {
                    relative.push(path.strip_prefix(source)?.to_owned());
                }
            }
            return Ok(Self {
                cwd: source.to_owned(),
                relative,
            });
        }
        let parent = source
            .parent()
            .with_context(|| format!("{} has no parent", source.display()))?;
        let name = source
            .file_name()
            .with_context(|| format!("{} has no file name", source.display()))?;
        Ok(Self {
            cwd: parent.to_owned(),
            relative: vec![PathBuf::from(name)],
        })
    }

    pub fn joined(&self, separator: u8) -> Vec<u8> {
        let names: Vec<&[u8]> = self
            .relative
            .iter()
            .map(|path| path.as_os_str().as_bytes())
            .collect();
        names.join(&separator)
    }
}
