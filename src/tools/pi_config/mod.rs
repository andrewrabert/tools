mod apply;
mod cli;
mod export;
mod spec;

use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode, ExitStatus};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::atomic_write;
use crate::dirs;
use crate::tools::Tool;
use crate::tools::pi_config::cli::Command;
pub use crate::tools::pi_config::cli::PiConfig;
use crate::tools::pi_config::spec::Config;

const TEMP_PREFIX: &str = ".pi-config-";

/// A file this tool creates may hold provider API keys, so only its owner can read it.
const NEW_FILE_MODE: u32 = 0o600;

impl Tool for PiConfig {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: PiConfig) -> Result<ExitCode> {
    match args.command {
        Command::Apply { dir, host } => apply::run(
            spec_dir(&dir)?,
            &host.resolve()?,
            &agent_dir()?,
            &mut install,
        ),
        Command::Export { dir, force } => {
            export::run(&dir, force, &agent_dir()?).map(|()| ExitCode::SUCCESS)
        }
        Command::Validate { dir, host } => {
            Config::load(spec_dir(&dir)?, &host.resolve()?).map(|_| ExitCode::SUCCESS)
        }
    }
}

fn spec_dir(dir: &Path) -> Result<&Path> {
    if !dir.is_dir() {
        bail!("not a directory: {}", dir.display());
    }
    Ok(dir)
}

fn agent_dir() -> Result<PathBuf> {
    Ok(dirs::home()?.join(".pi").join("agent"))
}

fn install(source: &str) -> Result<ExitStatus> {
    Process::new("pi")
        .args(["install", source])
        .status()
        .context("running pi")
}

/// Replaces `path` atomically, keeping the permissions of the file it replaces.
fn write(path: &Path, data: &[u8]) -> Result<()> {
    let permissions = match fs::metadata(path) {
        Ok(existing) => existing.permissions(),
        Err(_) => Permissions::from_mode(NEW_FILE_MODE),
    };
    let temp = atomic_write::sibling(TEMP_PREFIX, path)?;
    fs::write(&temp, data).with_context(|| format!("writing {}", temp.display()))?;
    fs::set_permissions(&temp, permissions)
        .with_context(|| format!("setting permissions of {}", temp.display()))?;
    atomic_write::replace(temp, path)
}

fn write_json(path: &Path, value: &Map<String, Value>) -> Result<()> {
    write(
        path,
        (serde_json::to_string_pretty(value)? + "\n").as_bytes(),
    )
}
