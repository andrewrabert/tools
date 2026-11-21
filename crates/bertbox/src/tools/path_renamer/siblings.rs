use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub enum Collisions {
    Reject,
    Suffix,
}

pub struct Siblings {
    parent: PathBuf,
    names: Vec<OsString>,
}

pub struct Rename {
    pub source: PathBuf,
    pub target: PathBuf,
}

impl Siblings {
    pub fn resolve(paths: &[PathBuf]) -> Result<Self> {
        let mut parents = BTreeSet::new();
        let mut names = Vec::new();
        for path in paths {
            let name = path
                .file_name()
                .with_context(|| format!("no file name: {}", path.display()))?;
            // Mount points such as /media/DIR are often links to /run/media/USER/DIR.
            let parent = match path.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent,
                _ => Path::new("."),
            };
            let parent = fs::canonicalize(parent)
                .with_context(|| format!("resolving {}", parent.display()))?;
            parents.insert(parent);
            names.push(name.to_owned());
        }

        let mut parents = parents.into_iter();
        let (Some(parent), None) = (parents.next(), parents.next()) else {
            bail!("varying parents");
        };
        if BTreeSet::from_iter(&names).len() != names.len() {
            bail!("duplicate sources");
        }
        Ok(Siblings { parent, names })
    }

    pub fn names(&self) -> &[OsString] {
        &self.names
    }

    pub fn renames(&self, targets: &[OsString], collisions: &Collisions) -> Result<Vec<Rename>> {
        if let Collisions::Reject = collisions
            && BTreeSet::from_iter(targets).len() != targets.len()
        {
            bail!("duplicate targets");
        }
        if self.names.len() != targets.len() {
            bail!("num sources does not match num targets");
        }
        let renames = self
            .names
            .iter()
            .zip(targets)
            .filter(|(source, target)| source != target)
            .map(|(source, target)| Rename {
                source: self.parent.join(source),
                target: self.parent.join(target),
            })
            .collect();
        Ok(renames)
    }
}

impl Rename {
    pub fn apply(self, collisions: &Collisions) -> Result<()> {
        let mut target = self.target;
        while target.exists() {
            match collisions {
                Collisions::Reject => bail!(
                    "target exists: {:?} -> {:?}",
                    self.source.display(),
                    target.display()
                ),
                Collisions::Suffix => target = suffixed(&target),
            }
        }
        fs::rename(&self.source, &target)
            .with_context(|| format!("renaming {} to {}", self.source.display(), target.display()))
    }
}

fn suffixed(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(OsStr::to_owned).unwrap_or_default();
    name.push("_");
    path.with_file_name(name)
}
