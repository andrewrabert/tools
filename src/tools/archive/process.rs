use std::env;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

pub fn installed(program: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|dir| {
        dir.join(program)
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    })
}

pub fn require(program: &str) -> Result<()> {
    if !installed(program) {
        bail!("{program} is not installed");
    }
    Ok(())
}

pub fn prefixed(prefix: &str, value: &OsStr) -> OsString {
    let mut argument = OsString::from(prefix);
    argument.push(value);
    argument
}

pub fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("running {command:?}"))?;
    if !status.success() {
        bail!("{command:?} failed: {status}");
    }
    Ok(())
}

pub fn capture(command: &mut Command) -> Result<Vec<u8>> {
    let output = command
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {command:?}"))?
        .wait_with_output()
        .with_context(|| format!("waiting for {command:?}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        match detail.trim() {
            "" => bail!("{command:?} failed: {}", output.status),
            detail => bail!("{command:?} failed: {}: {detail}", output.status),
        }
    }
    Ok(output.stdout)
}

pub fn feed(command: &mut Command, input: &[u8]) -> Result<()> {
    let mut child = command
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {command:?}"))?;
    let mut stdin = child
        .stdin
        .take()
        .with_context(|| format!("{command:?} has no stdin"))?;
    let written = stdin.write_all(input);
    drop(stdin);
    let status = child
        .wait()
        .with_context(|| format!("waiting for {command:?}"))?;
    written.with_context(|| format!("writing to {command:?}"))?;
    if !status.success() {
        bail!("{command:?} failed: {status}");
    }
    Ok(())
}
