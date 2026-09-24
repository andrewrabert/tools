use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use nix::unistd::{self, AccessFlags};

use crate::tools::dotfiles_link_bin::roots::Roots;

const HOSTS_DIR: &str = "scripts/hosts";
const COMMANDS_DIR: &str = "scripts/commands";
const TERMINAL_DIR: &str = "scripts/terminal";
const MCP_DIR: &str = "scripts/mcp";
const MEDIA_DIR: &str = "scripts/media";
const NOTED_BIN: &str = ".local/noted/bin";
const ZOEKT_SIMPLE_BIN: &str = ".local/zoekt-simple/bin";

/// Link sources in precedence order: a later source overrides an earlier one.
pub fn collect(roots: &Roots, host: &OsStr, dest: &Path) -> Result<Vec<PathBuf>> {
    let mut sources = vec![roots.primary.join(HOSTS_DIR).join(host)];
    sources.extend(
        roots
            .extras
            .iter()
            .map(|root| root.join(HOSTS_DIR).join(host)),
    );

    let installed = installed_commands(dest)?;
    let dir = roots.primary.join(COMMANDS_DIR);
    let entries = fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        if installed.contains(&entry.file_name()) {
            sources.push(entry.path());
        }
    }

    sources.push(roots.primary.join(TERMINAL_DIR));
    sources.push(roots.primary.join(NOTED_BIN));
    sources.extend(roots.extras.iter().map(|root| root.join(TERMINAL_DIR)));
    sources.extend(host_dirs(host).iter().map(|dir| roots.primary.join(dir)));
    Ok(sources)
}

fn host_dirs(host: &OsStr) -> &'static [&'static str] {
    match host.to_str() {
        Some("aweber") => &[MCP_DIR, ZOEKT_SIMPLE_BIN],
        Some("mars" | "sol") => &[MEDIA_DIR, ZOEKT_SIMPLE_BIN],
        Some("phobos") => &[MCP_DIR, MEDIA_DIR, ZOEKT_SIMPLE_BIN],
        _ => &[],
    }
}

fn installed_commands(dest: &Path) -> Result<HashSet<OsString>> {
    let path = env::var_os("PATH").context("PATH is not set")?;
    let mut commands = HashSet::new();
    for dir in env::split_paths(&path).filter(|dir| dir.is_dir() && dir != dest) {
        let entries = fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
            if is_executable_file(&entry.path()) {
                commands.insert(entry.file_name());
            }
        }
    }
    Ok(commands)
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file() && unistd::access(path, AccessFlags::X_OK).is_ok()
}
