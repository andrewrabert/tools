use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;

use crate::tools::archive::extract::backend::Request;
use crate::tools::archive::extract::listing::{self, Contents};
use crate::tools::archive::process;

const HEADER_LINES: usize = 1;
const FOOTER_LINES: usize = 2;
const NAME_FIELD: usize = 1;

pub fn contents(archive: &Path) -> Result<Contents> {
    let output = process::capture(Command::new("unshield").args(["l", "--"]).arg(archive))?;
    let lines = listing::lines(&output);
    let listed = lines
        .get(HEADER_LINES..lines.len().saturating_sub(FOOTER_LINES))
        .unwrap_or_default();
    Ok(Contents::named(
        listed
            .iter()
            .filter_map(|line| listing::split_fields(line.trim_ascii(), NAME_FIELD).pop())
            .map(|name| {
                name.iter()
                    .map(|byte| if *byte == b'\\' { b'/' } else { *byte })
                    .collect()
            }),
    ))
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    request.keeping_existing()?;
    process::run(
        Command::new("unshield")
            .args(["x", "-d"])
            .arg(request.directory()?)
            .arg("--")
            .arg(request.archive)
            .stdout(Stdio::null()),
    )
}
