use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;

use crate::tools::archive::extract::backend::Request;
use crate::tools::archive::extract::listing::{self, Contents};
use crate::tools::archive::process;

pub fn contents(archive: &Path) -> Result<Contents> {
    let output = process::capture(
        Command::new("unsquashfs")
            .arg("-lc")
            .arg(archive)
            .stderr(Stdio::null()),
    )?;
    Ok(Contents::named(
        listing::lines(&output).into_iter().map(<[u8]>::to_vec),
    ))
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    request.without_password()?;
    process::run(
        Command::new("unsquashfs")
            // unsquashfs refuses an existing destination, even an empty one, without -force
            .args(["-quiet", "-force", "-dest"])
            .arg(request.directory()?)
            .arg(request.archive),
    )
}
