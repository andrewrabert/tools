use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::tools::bertbox::packages::{GitHubRepository, PackageName};
use crate::which;

pub fn deploy(name: PackageName, repository: GitHubRepository) -> Result<PathBuf> {
    let GitHubRepository { owner, name: repo } = repository;
    let prefix = env::var_os("PREFIX").context("PREFIX is not set")?;
    let source = PathBuf::from(prefix)
        .join("etc/apt/sources.list.d")
        .join(format!("{name}.list"));
    if !source.exists() {
        if let Some(parent) = source.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(
            &source,
            format!("deb [trusted=yes] https://{owner}.github.io/{repo} stable main\n"),
        )
        .with_context(|| format!("writing {}", source.display()))?;
    }
    let status = Command::new("pkg")
        .args(["install", "-y", name.as_str()])
        .status()
        .context("running pkg")?;
    if !status.success() {
        bail!("pkg install {name} failed: {status}");
    }
    which::first(name.as_str())
        .with_context(|| format!("{name} is not on PATH after installing it"))
}
