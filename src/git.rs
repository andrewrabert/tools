use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

#[derive(Clone, Copy)]
pub struct Git<'a> {
    dir: Option<&'a Path>,
}

impl<'a> Git<'a> {
    pub fn cwd() -> Self {
        Self { dir: None }
    }

    pub fn at(dir: &'a Path) -> Self {
        Self { dir: Some(dir) }
    }

    pub fn command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new("git");
        if let Some(dir) = self.dir {
            command.arg("-C").arg(dir);
        }
        command.args(args);
        command
    }

    pub fn run<I, S>(&self, args: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        run(&mut self.command(args))
    }

    pub fn capture<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        capture(&mut self.command(args))
    }

    pub fn succeeds<I, S>(&self, args: I) -> Result<bool>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.command(args);
        let status = command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("running {command:?}"))?;
        Ok(status.success())
    }

    pub fn toplevel(&self) -> Result<String> {
        Ok(self
            .capture(["rev-parse", "--show-toplevel"])?
            .trim_end_matches('\n')
            .to_owned())
    }
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

pub fn capture(command: &mut Command) -> Result<String> {
    let output = command
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {command:?}"))?
        .wait_with_output()
        .with_context(|| format!("waiting for {command:?}"))?;
    if !output.status.success() {
        bail!("{command:?} failed: {}", output.status);
    }
    String::from_utf8(output.stdout).with_context(|| format!("reading the output of {command:?}"))
}
