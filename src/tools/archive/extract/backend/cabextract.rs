use std::path::Path;
use std::process::Command;

use anyhow::Result;

use crate::tools::archive::extract::backend::Request;
use crate::tools::archive::extract::listing::{self, Contents};
use crate::tools::archive::process;

const HEADER_LINES: usize = 2;
const NAME_FIELD: usize = 6;

pub fn contents(archive: &Path) -> Result<Contents> {
    let output = process::capture(
        Command::new("cabextract")
            .args(["--list", "--quiet", "--"])
            .arg(archive),
    )?;
    Ok(Contents::named(
        listing::lines(&output)
            .into_iter()
            .skip(HEADER_LINES)
            .filter_map(|line| listing::split_fields(line, NAME_FIELD).pop())
            .map(<[u8]>::to_vec),
    ))
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    request.keeping_existing()?;
    process::run(
        Command::new("cabextract")
            .args(["--quiet", "--directory"])
            .arg(request.directory()?)
            .arg("--")
            .arg(request.archive),
    )
}
