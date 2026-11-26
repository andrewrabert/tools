use std::ffi::{OsStr, OsString};
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

    pub fn tree_files(&self, revision: &str, paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
        let listed = self.capture(
            ["ls-tree", "-r", "-z", "--name-only", revision]
                .map(OsString::from)
                .into_iter()
                .chain(pathspec(paths)),
        )?;
        Ok(nul_separated(&listed))
    }

    pub fn file(&self, revision: &str, path: &Path) -> Result<String> {
        let object = format!("{revision}:./{}", path.display());
        self.capture(["cat-file", "blob", object.as_str()])
    }

    pub fn working_tree_files(&self, paths: &[PathBuf], ignored: Ignored) -> Result<Vec<PathBuf>> {
        let mut ls_files = vec!["ls-files", "-z", "--cached", "--others"];
        if let Ignored::Exclude = ignored {
            ls_files.push("--exclude-standard");
        }
        let listed = self.capture(
            ls_files
                .into_iter()
                .map(OsString::from)
                .chain(pathspec(paths)),
        )?;
        Ok(nul_separated(&listed))
    }

    pub fn merge_base(&self, a: &str, b: &str) -> Result<String> {
        Ok(self
            .capture(["merge-base", a, b])?
            .trim_end_matches('\n')
            .to_owned())
    }
}

#[derive(Clone, Copy)]
pub enum Ignored {
    Include,
    Exclude,
}

fn pathspec(paths: &[PathBuf]) -> impl Iterator<Item = OsString> {
    [OsString::from("--")]
        .into_iter()
        .chain(paths.iter().map(OsString::from))
}

fn nul_separated(listed: &str) -> Vec<PathBuf> {
    listed.split_terminator('\0').map(PathBuf::from).collect()
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
