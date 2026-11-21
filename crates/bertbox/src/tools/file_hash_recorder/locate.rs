use std::fs;
use std::path::{self, Path, PathBuf};

use anyhow::{Context, Result, bail};

const DATABASE_NAME: &str = "file_info.db";

#[derive(Clone, Copy)]
pub enum Missing {
    Fail,
    Create,
}

pub fn database_for(root: &Path, missing: Missing) -> Result<PathBuf> {
    let metadata = fs::metadata(root).with_context(|| format!("stat {}", root.display()))?;
    let root = path::absolute(root).with_context(|| format!("resolving {}", root.display()))?;
    if metadata.is_file() && root.file_name().is_some_and(|name| name == DATABASE_NAME) {
        return Ok(root);
    }

    let start = if metadata.is_dir() {
        root.as_path()
    } else {
        root.parent().unwrap_or(&root)
    };
    for dir in start.ancestors() {
        let candidate = dir.join(DATABASE_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    match missing {
        Missing::Fail => bail!(
            "Could not find {DATABASE_NAME} in {} or any parent directory",
            root.display()
        ),
        Missing::Create => Ok(start.join(DATABASE_NAME)),
    }
}
