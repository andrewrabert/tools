use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::tools::archive::extract::backend::{Request, file_name};
use crate::tools::archive::extract::config::Overwrite;
use crate::tools::archive::extract::listing::{self, Contents};
use crate::tools::archive::process;

const DISC_IMAGE_EXTENSION: &str = "gcm";
const NAME_FIELD: usize = 3;

pub fn gcm_contents(archive: &Path) -> Result<Contents> {
    let output = process::capture(
        Command::new("dolphin-tool")
            .args(["extract", "--list", "--input"])
            .arg(archive),
    )?;
    Ok(Contents::named(
        listing::lines(&output)
            .into_iter()
            .filter_map(|line| listing::split_fields(line, NAME_FIELD).pop())
            .map(<[u8]>::to_vec),
    ))
}

pub fn gcm_extract(request: &Request) -> Result<()> {
    request.every_member()?;
    process::run(
        Command::new("dolphin-tool")
            .args(["extract", "--quiet", "--input"])
            .arg(request.archive)
            .arg("--output")
            .arg(request.directory()?)
            .arg(request.archive)
            .stdin(Stdio::null())
            .stdout(Stdio::null()),
    )
}

fn disc_image_name(archive: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(file_name(archive)?).with_extension(DISC_IMAGE_EXTENSION))
}

pub fn rvz_contents(archive: &Path) -> Result<Contents> {
    Ok(Contents::named([disc_image_name(archive)?
        .as_os_str()
        .as_bytes()
        .to_owned()]))
}

pub fn rvz_extract(request: &Request) -> Result<()> {
    request.every_member()?;
    let target = request.directory()?.join(disc_image_name(request.archive)?);
    if target.is_file() {
        match request.overwrite {
            Overwrite::Replace => fs::remove_file(&target)
                .with_context(|| format!("removing {}", target.display()))?,
            Overwrite::Keep => bail!("{} already exists", target.display()),
        }
    }
    process::run(
        Command::new("dolphin-tool")
            .args(["convert", "--format=iso"])
            .arg(process::prefixed("--input=", request.archive.as_os_str()))
            .arg(process::prefixed("--output=", target.as_os_str()))
            .stdout(Stdio::null()),
    )
}
