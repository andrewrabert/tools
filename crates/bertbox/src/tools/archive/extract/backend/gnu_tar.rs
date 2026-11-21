use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::tools::archive::extract::backend::{Destination, Request};
use crate::tools::archive::extract::config::Overwrite;
use crate::tools::archive::extract::listing::{self, Contents};
use crate::tools::archive::process;

pub fn check() -> Result<()> {
    let version = process::capture(Command::new("tar").arg("--version"))?;
    let is_gnu = listing::lines(&version)
        .first()
        .is_some_and(|line| String::from_utf8_lossy(line).contains("GNU tar"));
    if !is_gnu {
        bail!("tar is not GNU tar");
    }
    Ok(())
}

pub fn contents(archive: &Path) -> Result<Contents> {
    let output = process::capture(
        Command::new("tar")
            .arg("tf")
            .arg(archive)
            .arg("--force-local")
            .stderr(Stdio::null()),
    )?;
    Ok(Contents::named(
        listing::lines(&output).into_iter().map(<[u8]>::to_vec),
    ))
}

pub fn extract(request: &Request) -> Result<()> {
    let mut command = Command::new("tar");
    command.arg("xf").arg(request.archive).arg("--force-local");
    match request.destination {
        Destination::Stream(fd) => {
            let output = fd.try_clone().context("duplicating the output stream")?;
            command.arg("--to-stdout").stdout(output)
        }
        Destination::Directory(directory) => {
            command.arg(process::prefixed("--one-top-level=", directory.as_os_str()))
        }
    };
    command.arg(match request.overwrite {
        Overwrite::Replace => "--overwrite",
        Overwrite::Keep => "--keep-old-files",
    });
    process::run(command.args(request.members))
}
