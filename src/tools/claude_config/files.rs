//! Files written only when their bytes change, and directory trees kept as exact copies.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{MetadataExt, chown};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use similar::TextDiff;
use walkdir::WalkDir;

use crate::atomic_write;

const TEMP_PREFIX: &str = ".claude-config-";
const YAML_SUFFIX: &str = ".yaml";

/// Progress written to stderr; only warnings are written when quiet.
#[derive(Clone, Copy)]
pub struct Log {
    pub quiet: bool,
}

impl Log {
    pub fn info(self, message: impl Display) {
        if !self.quiet {
            eprintln!("{message}");
        }
    }

    pub fn warn(self, message: impl Display) {
        eprintln!("{message}");
    }
}

pub fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(data) => Ok(Some(data)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

pub fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let existing = fs::metadata(path).ok();
    let temp = atomic_write::sibling(TEMP_PREFIX, path)?;
    fs::write(&temp, data).with_context(|| format!("writing {}", temp.display()))?;
    if let Some(existing) = existing {
        // Changing either id needs privileges this process may lack; the write still stands.
        let _ = chown(&temp, Some(existing.uid()), None);
        let _ = chown(&temp, None, Some(existing.gid()));
        fs::set_permissions(&temp, existing.permissions())
            .with_context(|| format!("setting permissions of {}", temp.display()))?;
    }
    atomic_write::replace(temp, path)
}

pub fn write_if_different(path: &Path, data: &[u8], log: Log) -> Result<()> {
    if read_optional(path)?.as_deref() == Some(data) {
        return Ok(());
    }
    write_atomic(path, data)?;
    log.info(format_args!("wrote {}", path.display()));
    Ok(())
}

pub fn sync_file(dest: &Path, data: Option<&[u8]>, log: Log) -> Result<()> {
    match data {
        Some(data) => write_if_different(dest, data, log),
        None => remove_if_exists(dest),
    }
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("removing {}", path.display())),
    }
}

/// Every file under `root` by its path relative to `root`; symlinked directories are not entered.
fn tree_files(root: &Path) -> Result<BTreeMap<PathBuf, PathBuf>> {
    let mut files = BTreeMap::new();
    if !root.exists() {
        return Ok(files);
    }
    for entry in WalkDir::new(root).min_depth(1) {
        let entry = entry.with_context(|| format!("walking {}", root.display()))?;
        let file_type = entry.file_type();
        if file_type.is_file() || (file_type.is_symlink() && entry.path().is_file()) {
            let relative = entry.path().strip_prefix(root)?.to_owned();
            files.insert(relative, entry.into_path());
        }
    }
    Ok(files)
}

fn prune_empty_dirs(root: &Path) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in WalkDir::new(root).contents_first(true) {
        let entry = entry.with_context(|| format!("walking {}", root.display()))?;
        if !entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        let mut children =
            fs::read_dir(path).with_context(|| format!("reading {}", path.display()))?;
        if children.next().is_none() {
            fs::remove_dir(path).with_context(|| format!("removing {}", path.display()))?;
        }
    }
    Ok(())
}

pub fn mirror_tree(source: &Path, dest: &Path, dry_run: bool, log: Log) -> Result<()> {
    let source_files = tree_files(source)?;
    let dest_files = tree_files(dest)?;

    for (relative, source_path) in &source_files {
        let data =
            fs::read(source_path).with_context(|| format!("reading {}", source_path.display()))?;
        let dest_path = dest.join(relative);
        if read_optional(&dest_path)?.as_ref() == Some(&data) {
            continue;
        }
        if dry_run {
            log.info(format_args!("would write {}", dest_path.display()));
            continue;
        }
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        write_atomic(&dest_path, &data)?;
        let permissions = fs::metadata(source_path)
            .with_context(|| format!("reading {}", source_path.display()))?
            .permissions();
        fs::set_permissions(&dest_path, permissions)
            .with_context(|| format!("setting permissions of {}", dest_path.display()))?;
        log.info(format_args!("wrote {}", dest_path.display()));
    }

    for (relative, dest_path) in &dest_files {
        if source_files.contains_key(relative) {
            continue;
        }
        if dry_run {
            log.info(format_args!("would remove {}", dest_path.display()));
            continue;
        }
        fs::remove_file(dest_path).with_context(|| format!("removing {}", dest_path.display()))?;
        log.info(format_args!("removed {}", dest_path.display()));
    }

    if !dry_run {
        prune_empty_dirs(dest)?;
    }
    Ok(())
}

pub fn yaml_files(dir: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut files = BTreeMap::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(files),
        Err(error) => return Err(error).with_context(|| format!("reading {}", dir.display())),
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let file_name = entry.file_name();
        let Some(name) = file_name
            .to_str()
            .and_then(|name| name.strip_suffix(YAML_SUFFIX))
        else {
            continue;
        };
        files.insert(name.to_owned(), entry.path());
    }
    Ok(files)
}

pub fn mirror_yaml_dir(
    dir: &Path,
    documents: &BTreeMap<String, String>,
    dry_run: bool,
    log: Log,
) -> Result<()> {
    let existing = yaml_files(dir)?;
    for (name, text) in documents {
        let path = dir.join(format!("{name}{YAML_SUFFIX}"));
        if dry_run {
            if read_optional(&path)?.as_deref() != Some(text.as_bytes()) {
                log.info(format_args!("would write {}", path.display()));
            }
            continue;
        }
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        write_if_different(&path, text.as_bytes(), log)?;
    }
    for (name, path) in &existing {
        if documents.contains_key(name) {
            continue;
        }
        if dry_run {
            log.info(format_args!("would remove {}", path.display()));
            continue;
        }
        fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
        log.info(format_args!("removed {}", path.display()));
    }
    if !dry_run {
        prune_empty_dirs(dir)?;
    }
    Ok(())
}

pub fn unified_diff(name: &str, current: &str, desired: &str) -> String {
    TextDiff::from_lines(current, desired)
        .unified_diff()
        .header(name, name)
        .to_string()
}

pub fn yaml_dir_diff(
    dir: &Path,
    label: &str,
    documents: &BTreeMap<String, String>,
) -> Result<String> {
    let existing = yaml_files(dir)?;
    let names: BTreeSet<&String> = documents.keys().chain(existing.keys()).collect();
    let mut diff = String::new();
    for name in names {
        let baseline = match existing.get(name) {
            Some(path) => read_text(path)?,
            None => String::new(),
        };
        let desired = documents.get(name).map(String::as_str).unwrap_or_default();
        diff.push_str(&unified_diff(
            &format!("{label}/{name}{YAML_SUFFIX}"),
            &baseline,
            desired,
        ));
    }
    Ok(diff)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;

    const LOG: Log = Log { quiet: true };

    fn write(path: &Path, data: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }

    #[test]
    fn mirror_tree_copies_changes_and_removes_extras() {
        let temp = tempfile::tempdir().unwrap();
        let (source, dest) = (temp.path().join("source"), temp.path().join("dest"));
        write(&source.join("a.md"), "a");
        write(&source.join("sub/b.sh"), "b");
        fs::set_permissions(source.join("sub/b.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        write(&dest.join("a.md"), "stale");
        write(&dest.join("old/deep/c.md"), "c");

        mirror_tree(&source, &dest, false, LOG).unwrap();

        assert_eq!(fs::read_to_string(dest.join("a.md")).unwrap(), "a");
        assert_eq!(fs::read_to_string(dest.join("sub/b.sh")).unwrap(), "b");
        let mode = fs::metadata(dest.join("sub/b.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert!(!dest.join("old").exists());
    }

    #[test]
    fn mirror_tree_dry_run_changes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let (source, dest) = (temp.path().join("source"), temp.path().join("dest"));
        write(&source.join("a.md"), "a");
        write(&dest.join("b.md"), "b");

        mirror_tree(&source, &dest, true, LOG).unwrap();

        assert!(!dest.join("a.md").exists());
        assert!(dest.join("b.md").exists());
    }

    #[test]
    fn mirror_tree_of_missing_source_removes_dest() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path().join("dest");
        write(&dest.join("sub/a.md"), "a");

        mirror_tree(&temp.path().join("missing"), &dest, false, LOG).unwrap();

        assert!(!dest.exists());
    }

    #[test]
    fn tree_files_skips_symlinked_directories_and_reads_symlinked_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        write(&root.join("real/a.md"), "a");
        write(&temp.path().join("outside/b.md"), "b");
        symlink(temp.path().join("outside"), root.join("linked-dir")).unwrap();
        symlink(temp.path().join("outside/b.md"), root.join("linked.md")).unwrap();

        let files: Vec<PathBuf> = tree_files(&root).unwrap().into_keys().collect();

        assert_eq!(
            files,
            [PathBuf::from("linked.md"), PathBuf::from("real/a.md")]
        );
    }

    #[test]
    fn write_keeps_the_mode_of_the_file_it_replaces() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        write(&path, "old");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        write_if_different(&path, b"new", LOG).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn mirror_yaml_dir_writes_documents_and_prunes_the_rest() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("mcp");
        write(&dir.join("stale.yaml"), "x: 1\n");
        write(&dir.join("notes.txt"), "kept");
        let documents = BTreeMap::from([("fresh".to_owned(), "y: 2\n".to_owned())]);

        mirror_yaml_dir(&dir, &documents, false, LOG).unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("fresh.yaml")).unwrap(),
            "y: 2\n"
        );
        assert!(!dir.join("stale.yaml").exists());
        assert!(dir.join("notes.txt").exists());

        mirror_yaml_dir(&dir, &BTreeMap::new(), false, LOG).unwrap();
        assert!(dir.join("notes.txt").exists());
    }

    #[test]
    fn unified_diff_is_empty_for_equal_text() {
        assert_eq!(unified_diff("a", "x\n", "x\n"), "");
        assert_eq!(
            unified_diff("a", "x\n", "y\n"),
            "--- a\n+++ a\n@@ -1 +1 @@\n-x\n+y\n"
        );
    }
}
