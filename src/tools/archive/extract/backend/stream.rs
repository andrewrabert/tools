use std::fs::File;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::tools::archive::extract::backend::{Destination, Request, file_stem};
use crate::tools::archive::extract::config::Overwrite;
use crate::tools::archive::extract::listing::Contents;
use crate::tools::archive::process;

pub fn contents(archive: &Path) -> Result<Contents> {
    Ok(Contents::named([file_stem(archive)?.as_bytes().to_owned()]))
}

fn to_destination(command: &mut Command, request: &Request) -> Result<()> {
    request.every_member()?;
    match request.destination {
        Destination::Directory(directory) => {
            let target = directory.join(file_stem(request.archive)?);
            let output =
                File::create(&target).with_context(|| format!("opening {}", target.display()))?;
            command.stdout(output);
        }
        Destination::Stream(fd) => {
            let output = fd.try_clone().context("duplicating the output stream")?;
            command.stdout(output);
        }
    }
    process::run(command)
}

pub fn gzip(request: &Request) -> Result<()> {
    request.keeping_existing()?;
    to_destination(
        Command::new("gzip")
            .args(["-d", "-c", "--"])
            .arg(request.archive),
        request,
    )
}

pub fn pigz(request: &Request) -> Result<()> {
    request.keeping_existing()?;
    to_destination(
        Command::new("pigz")
            .args(["-d", "-k", "-c", "--"])
            .arg(request.archive),
        request,
    )
}

pub fn bzip2(request: &Request) -> Result<()> {
    request.keeping_existing()?;
    to_destination(
        Command::new("bzip2")
            .args(["-d", "-c", "--"])
            .arg(request.archive),
        request,
    )
}

pub fn pbzip2(request: &Request) -> Result<()> {
    request.keeping_existing()?;
    to_destination(
        Command::new("pbzip2")
            .args(["-d", "-c", "--"])
            .arg(request.archive),
        request,
    )
}

pub fn xz(request: &Request) -> Result<()> {
    request.keeping_existing()?;
    to_destination(
        Command::new("xz")
            .args(["-d", "-c", "--"])
            .arg(request.archive),
        request,
    )
}

pub fn brotli(request: &Request) -> Result<()> {
    let mut command = Command::new("brotli");
    command.args(["--decompress", "--stdout"]);
    if let Overwrite::Replace = request.overwrite {
        command.arg("--force");
    }
    to_destination(command.arg("--").arg(request.archive), request)
}

pub fn pixz(request: &Request) -> Result<()> {
    request.every_member()?;
    request.keeping_existing()?;
    let target = request.directory()?.join(file_stem(request.archive)?);
    process::run(
        Command::new("pixz")
            .args(["-d", "-k", "--"])
            .arg(request.archive)
            .arg(target),
    )
}

pub fn lz4(request: &Request) -> Result<()> {
    request.every_member()?;
    let target = request.directory()?.join(file_stem(request.archive)?);
    let mut command = Command::new("lz4");
    command.arg("-d");
    if let Overwrite::Replace = request.overwrite {
        command.arg("-f");
    }
    process::run(
        command
            .arg("--")
            .arg(request.archive)
            .arg(target)
            .stdin(Stdio::null()),
    )
}

pub fn zstd(request: &Request) -> Result<()> {
    request.every_member()?;
    let mut command = Command::new("zstd");
    command
        .args(["-d", "--quiet", "--output-dir-flat"])
        .arg(request.directory()?);
    if let Overwrite::Replace = request.overwrite {
        command.arg("-f");
    }
    process::run(command.arg("--").arg(request.archive))
}
