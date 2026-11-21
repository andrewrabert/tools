use std::ffi::OsStr;
use std::path::{Path, PathBuf};
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

    pub fn capture<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.command(args);
        let output = command
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("running {command:?}"))?
            .wait_with_output()
            .with_context(|| format!("waiting for {command:?}"))?;
        if !output.status.success() {
            bail!("{command:?} failed: {}", output.status);
        }
        String::from_utf8(output.stdout)
            .with_context(|| format!("reading the output of {command:?}"))
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

    pub fn toplevel(&self) -> Result<PathBuf> {
        Ok(PathBuf::from(
            self.capture(["rev-parse", "--show-toplevel"])?
                .trim_end_matches('\n'),
        ))
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
