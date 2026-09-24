use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{self, Path, PathBuf};

use anyhow::{Context, Result};

pub struct Links(BTreeMap<OsString, PathBuf>);

impl Links {
    pub fn from_sources(sources: &[PathBuf]) -> Result<Self> {
        let mut links = BTreeMap::new();
        for source in sources.iter().filter(|source| source.exists()) {
            let paths = if source.is_file() {
                vec![source.clone()]
            } else {
                entries_of(source)?
            };
            for path in paths {
                let Some(name) = path.file_name() else {
                    continue;
                };
                links.insert(name.to_owned(), link_target(&path)?);
            }
        }
        Ok(Links(links))
    }

    pub fn install(&self, dest: &Path) -> Result<()> {
        for stale in entries_of(dest)? {
            let linked = stale
                .file_name()
                .is_some_and(|name| self.0.contains_key(name));
            if !linked {
                fs::remove_file(&stale).with_context(|| format!("removing {}", stale.display()))?;
            }
        }
        for (name, target) in &self.0 {
            point(&dest.join(name), target)?;
        }
        Ok(())
    }
}

/// Make `link` a symlink to `target`, replacing any file already there.
pub fn point(link: &Path, target: &Path) -> Result<()> {
    if fs::read_link(link).is_ok_and(|current| current == target) {
        return Ok(());
    }
    if link.symlink_metadata().is_ok() {
        fs::remove_file(link).with_context(|| format!("removing {}", link.display()))?;
    }
    symlink(target, link)
        .with_context(|| format!("linking {} to {}", link.display(), target.display()))
}

fn link_target(path: &Path) -> Result<PathBuf> {
    let target = if path.is_symlink() {
        let target =
            fs::read_link(path).with_context(|| format!("reading link {}", path.display()))?;
        match path.parent() {
            Some(parent) => parent.join(target),
            None => target,
        }
    } else {
        path.to_owned()
    };
    path::absolute(&target).with_context(|| format!("resolving {}", target.display()))
}

fn entries_of(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        paths.push(entry.path());
    }
    Ok(paths)
}
