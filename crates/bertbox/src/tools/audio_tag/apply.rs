use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;

use crate::tools::audio_tag::json::{FileUpdates, Update};
use crate::tools::audio_tag::tags::Tags;

#[derive(Clone, Copy)]
pub enum Mode {
    /// Tags become exactly the given ones.
    Replace,
    /// Given tags are set or, when null, deleted; the rest are kept.
    Merge,
    /// Like merge, with the changes given on the command line.
    Set,
}

impl Mode {
    fn verbs(self) -> (&'static str, &'static str) {
        match self {
            Mode::Replace => ("Replacing", "Replaced"),
            Mode::Merge => ("Merging", "Merged"),
            Mode::Set => ("Setting", "Set"),
        }
    }
}

pub fn apply(files: &[FileUpdates], mode: Mode) -> Result<()> {
    let (verb, past) = mode.verbs();
    let mut changed = 0;
    let mut unchanged = 0;
    for file in files {
        if !file.path.exists() {
            eprintln!("File not found: {}", file.path.display());
            continue;
        }
        let current = Tags::read(&file.path)?;
        let new = match mode {
            Mode::Replace => file
                .updates
                .iter()
                .filter_map(|(name, update)| match update {
                    Update::Set(values) => Some((name.clone(), values.clone())),
                    Update::Delete => None,
                })
                .collect(),
            Mode::Merge | Mode::Set => merged(&current, &file.updates),
        }
        .sorted();
        if !report(&file.path, verb, &current, &new) {
            unchanged += 1;
            continue;
        }
        new.write(&file.path)?;
        changed += 1;
    }
    eprintln!("\n{past} {changed} files, {unchanged} unchanged");
    Ok(())
}

fn merged(current: &Tags, updates: &[(String, Update)]) -> Tags {
    let mut tags = current.clone();
    for (name, update) in updates {
        match update {
            Update::Set(values) => tags.set(name, values.clone()),
            Update::Delete => {
                tags.remove(name);
            }
        }
    }
    tags
}

/// Print each changed tag; false when nothing changed.
fn report(path: &Path, verb: &str, current: &Tags, new: &Tags) -> bool {
    let (current, new) = (current.by_name(), new.by_name());
    if current == new {
        return false;
    }
    eprintln!("{verb}: {}", path.display());
    let names: BTreeSet<&str> = current.keys().chain(new.keys()).copied().collect();
    for name in names {
        let (old, new) = (current.get(name), new.get(name));
        if old == new {
            continue;
        }
        let old = old.map_or_else(|| "None".to_owned(), |values| format!("{values:?}"));
        let new = new.map_or_else(|| "[deleted]".to_owned(), |values| format!("{values:?}"));
        eprintln!("  {}: {old} -> {new}", name.to_uppercase());
    }
    true
}
