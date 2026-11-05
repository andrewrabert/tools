//! Executables found on PATH.
use std::env;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn is_executable(candidate: &Path) -> bool {
    candidate
        .metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Every executable named `program` on PATH, in PATH order.
pub fn all(program: &str) -> Vec<PathBuf> {
    let path = env::var_os("PATH").unwrap_or_default();
    env::split_paths(&path)
        .map(|dir| dir.join(program))
        .filter(|candidate| is_executable(candidate))
        .collect()
}

/// The first executable named `program` on PATH.
pub fn first(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}
