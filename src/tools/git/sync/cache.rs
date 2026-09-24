use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::tools::git::sync::source::Source;

pub struct RepoList {
    path: PathBuf,
    entries: Vec<String>,
    modified: bool,
}

impl RepoList {
    pub fn load(path: PathBuf, root: &Path) -> Result<Self> {
        let mut entries = Vec::new();
        let mut modified = false;
        for entry in read_lines(&path)? {
            let mut dir = root.join(&entry);
            if !dir.exists() {
                match entry.parse::<Source>() {
                    Ok(source) => dir = source.dir(root),
                    Err(_) => continue,
                }
            }
            if dir.is_dir() {
                entries.push(entry);
            } else {
                modified = true;
            }
        }
        Ok(RepoList {
            path,
            entries,
            modified,
        })
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn remove(&mut self, entry: &str) {
        if let Some(index) = self.entries.iter().position(|existing| existing == entry) {
            self.entries.remove(index);
            self.modified = true;
        }
    }

    pub fn promote(&mut self, entry: String) {
        self.remove(&entry);
        self.entries.push(entry);
        self.modified = true;
    }

    pub fn save(&self) -> Result<()> {
        if self.modified {
            write_lines(&self.path, &self.entries)?;
        }
        Ok(())
    }
}

/// The lines of a cache file, created empty when missing.
pub fn read_lines(path: &Path) -> Result<Vec<String>> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text.lines().map(str::to_owned).collect())
}

pub fn write_lines(path: &Path, lines: &[String]) -> Result<()> {
    fs::write(path, lines.join("\n")).with_context(|| format!("writing {}", path.display()))
}
