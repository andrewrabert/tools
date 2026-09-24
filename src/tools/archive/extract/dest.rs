use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::tools::archive::extract::config::Placement;

pub fn prepare(archive: &Path, parent: Option<&Path>, placement: &Placement) -> Result<PathBuf> {
    let parent = match parent {
        Some(parent) => parent,
        None => archive
            .parent()
            .with_context(|| format!("{} has no parent", archive.display()))?,
    };
    let dest = match placement {
        Placement::Merged => parent.to_owned(),
        Placement::ChildDirectory => {
            let mut name = child_name(archive)?;
            while parent.join(&name).exists() {
                name.push("_");
            }
            parent.join(name)
        }
    };
    fs::create_dir_all(&dest).with_context(|| format!("creating {}", dest.display()))?;
    Ok(dest)
}

fn child_name(archive: &Path) -> Result<OsString> {
    let stem = archive
        .file_stem()
        .with_context(|| format!("{} has no file name", archive.display()))?;
    let inner = Path::new(stem);
    let is_tarball = inner
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("tar"));
    match (is_tarball, inner.file_stem()) {
        (true, Some(inner_stem)) => Ok(inner_stem.to_owned()),
        _ => Ok(stem.to_owned()),
    }
}
