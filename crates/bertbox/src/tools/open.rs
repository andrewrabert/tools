use std::env;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;

use crate::dispatch;
use crate::tools::Tool;

const NAME: &str = "open";
const OPENERS: [&str; 2] = ["termux-open", "xdg-open"];

#[derive(ClapArgs)]
#[command(about = "Open files or URLs with the desktop opener, detached from the terminal")]
pub struct Open {
    #[arg(
        value_name = "ARG",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    args: Vec<OsString>,
}

impl Tool for Open {
    fn run(self) -> ExitCode {
        match open(&self.args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

pub fn open(args: &[OsString]) -> Result<()> {
    if let Some(opener) = OPENERS.into_iter().find_map(|name| which(name).next()) {
        return detach(opener, args);
    }
    let program = dispatch::program()?;
    let own = fs::canonicalize(&program).unwrap_or(program);
    let other = which(NAME)
        .find(|candidate| fs::canonicalize(candidate).is_ok_and(|candidate| candidate != own));
    match other {
        Some(other) => {
            let error = Command::new(&other).args(args).exec();
            Err(error).with_context(|| format!("running {}", other.display()))
        }
        None => bail!("command not found: {}", OPENERS.join(", ")),
    }
}

/// Runs `program` in its own process group with no terminal, like `nohup ... &`.
fn detach(program: PathBuf, args: &[OsString]) -> Result<()> {
    Command::new(&program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .with_context(|| format!("running {}", program.display()))?;
    Ok(())
}

/// Every executable named `program` on PATH, in PATH order.
fn which(program: &str) -> impl Iterator<Item = PathBuf> {
    let path = env::var_os("PATH").unwrap_or_default();
    env::split_paths(&path)
        .map(|dir| dir.join(program))
        .filter(|candidate| {
            candidate
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
        .collect::<Vec<_>>()
        .into_iter()
}
