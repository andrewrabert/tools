use std::path::Path;

use anyhow::Result;
use walkdir::{DirEntry, WalkDir};

/// Every entry under `root`, `root` included, descending through symlinked directories.
pub fn entries(root: &Path) -> impl Iterator<Item = Result<DirEntry>> {
    WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .map(|entry| entry.map_err(Into::into))
}
