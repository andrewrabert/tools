use std::env;
use std::fmt;
use std::iter;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Output, Stdio};

use anyhow::{Context, Result};

/// A command that exited unsuccessfully (or, where warnings count, wrote to stderr).
#[derive(Debug)]
pub struct ProcessFailed {
    command: String,
    status: ExitStatus,
    stderr: Option<String>,
}

impl ProcessFailed {
    fn new(command: &Command, status: ExitStatus, stderr: Option<String>) -> Self {
        ProcessFailed {
            command: describe(command),
            status,
            stderr,
        }
    }

    pub fn stderr(&self) -> Option<&str> {
        self.stderr.as_deref()
    }
}

impl fmt::Display for ProcessFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.status, self.command)
    }
}

impl std::error::Error for ProcessFailed {}

pub fn describe(command: &Command) -> String {
    iter::once(command.get_program())
        .chain(command.get_args())
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

fn running(command: &Command) -> String {
    format!("running {}", command.get_program().to_string_lossy())
}

fn nonempty(stderr: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

pub fn check(command: &Command, status: ExitStatus, stderr: Option<String>) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(ProcessFailed::new(command, status, stderr).into())
    }
}

/// Runs to completion with whatever stdio the command has configured.
pub fn run(command: &mut Command) -> Result<()> {
    let status = command.status().with_context(|| running(command))?;
    check(command, status, None)
}

/// Runs to completion and returns stdout; stderr stays as configured.
pub fn output(command: &mut Command) -> Result<Vec<u8>> {
    let child = command
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| running(command))?;
    let output = child.wait_with_output().with_context(|| running(command))?;
    check(command, output.status, nonempty(&output.stderr))?;
    Ok(output.stdout)
}

/// Runs to completion and returns stdout as text.
pub fn output_text(command: &mut Command) -> Result<String> {
    let stdout = output(command)?;
    String::from_utf8(stdout)
        .with_context(|| format!("{} wrote non-UTF-8 output", describe(command)))
}

/// Fails when the command exits unsuccessfully or writes anything to stderr.
pub fn strict(command: &Command, output: &Output) -> Result<()> {
    let stderr = nonempty(&output.stderr);
    if output.status.success() && stderr.is_none() {
        Ok(())
    } else {
        Err(ProcessFailed::new(command, output.status, stderr).into())
    }
}

/// Runs to completion, treating any stderr output as failure.
pub fn run_strict(command: &mut Command) -> Result<()> {
    let child = command
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| running(command))?;
    let output = child.wait_with_output().with_context(|| running(command))?;
    strict(command, &output)
}

/// The first executable named `program` on PATH.
pub fn which(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| {
            candidate
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}
