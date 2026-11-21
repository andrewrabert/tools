use std::fs;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::tools::archive::extract::backend::{Request, TEMP_PREFIX, file_name};
use crate::tools::archive::extract::listing::Contents;
use crate::tools::archive::process;
use crate::tools::archive::temp;

const EXTRACTED_EXTENSION: &str = "nsp";

fn extracted_name(archive: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(file_name(archive)?).with_extension(EXTRACTED_EXTENSION))
}

pub fn contents(archive: &Path) -> Result<Contents> {
    Ok(Contents::named([extracted_name(archive)?
        .as_os_str()
        .as_bytes()
        .to_owned()]))
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    request.keeping_existing()?;
    let directory = request.directory()?;
    let target = directory.join(extracted_name(request.archive)?);
    if target.exists() {
        bail!("{} already exists", target.display());
    }
    let scratch = temp::dir(directory, file_name(directory)?, TEMP_PREFIX)?;

    let mut command = Command::new("nsz");
    command
        .arg("-D")
        .arg(process::prefixed("--output=", scratch.path().as_os_str()))
        .arg(request.archive)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = command.spawn().context("running nsz")?;
    // When the prod keys are missing, nsz reports it and then waits for enter.
    let mut stdin = child.stdin.take().context("nsz has no stdin")?;
    let _ = stdin.write_all(b"\n");
    drop(stdin);
    let output = child.wait_with_output().context("waiting for nsz")?;
    if !output.status.success() {
        bail!(
            "{command:?} failed: {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }

    let entry = fs::read_dir(scratch.path())
        .with_context(|| format!("listing {}", scratch.path().display()))?
        .next()
        .context("nsz produced no file")?
        .with_context(|| format!("listing {}", scratch.path().display()))?;
    if target.exists() {
        bail!("{} already exists", target.display());
    }
    fs::rename(entry.path(), &target).with_context(|| format!("moving to {}", target.display()))
}
