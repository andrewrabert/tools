use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::tools::archive::extract::backend::Request;
use crate::tools::archive::extract::config::Overwrite;
use crate::tools::archive::extract::listing::{self, Contents};
use crate::tools::archive::process;

const VOLUME_PREFIX: &str = "Archive: ";

pub fn contents(archive: &Path) -> Result<Contents> {
    let output = process::capture(Command::new("unrar").args(["vb", "-v", "--"]).arg(archive))?;
    Ok(Contents::named(
        listing::lines(&output).into_iter().map(<[u8]>::to_vec),
    ))
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    let mut command = Command::new("unrar");
    command.args(["x", "-idc", "-idd", "-idn", "-idp", "-idq"]);
    if let Overwrite::Replace = request.overwrite {
        command.arg("-o+");
    }
    if let Some(password) = request.password {
        command.arg(format!("-p{}", password.as_str()));
    }
    process::run(
        command
            .arg("--")
            .arg(request.archive)
            .arg(request.directory()?)
            .stdin(Stdio::null()),
    )
}

pub fn volumes(archive: &Path) -> Result<Vec<PathBuf>> {
    let output = process::capture(Command::new("unrar").args(["vt", "-v", "--"]).arg(archive))?;
    let output = String::from_utf8(output).context("unrar listed a name that is not UTF-8")?;
    let parent = archive
        .parent()
        .with_context(|| format!("{} has no parent", archive.display()))?;
    Ok(output
        .lines()
        .filter_map(|line| line.strip_prefix(VOLUME_PREFIX))
        .map(|name| parent.join(name.trim_start()))
        .collect())
}
