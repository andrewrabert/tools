use std::path::Path;
use std::process::Command;

use anyhow::Result;

use crate::tools::archive::extract::backend::Request;
use crate::tools::archive::extract::listing::{self, Contents};
use crate::tools::archive::process;

pub fn contents(archive: &Path) -> Result<Contents> {
    let output = process::capture(
        Command::new("cpio")
            // --quiet drops the trailing block count
            .args(["--list", "--no-absolute-filenames", "--quiet", "--file"])
            .arg(archive),
    )?;
    Ok(Contents::named(
        listing::lines(&output).into_iter().map(<[u8]>::to_vec),
    ))
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    request.keeping_existing()?;
    process::run(
        Command::new("cpio")
            .args(["--extract", "--directory"])
            .arg(request.directory()?)
            .args([
                "--make-directories",
                "--no-absolute-filenames",
                "--preserve-modification-time",
                "--file",
            ])
            .arg(request.archive),
    )
}
