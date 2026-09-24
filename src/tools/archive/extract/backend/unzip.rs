use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;

use crate::tools::archive::extract::backend::Request;
use crate::tools::archive::extract::config::Overwrite;
use crate::tools::archive::extract::listing::{self, Contents};
use crate::tools::archive::process;

const NAME_FIELD: usize = 3;
// -O: the encoding of DOS, Windows and OS/2 names; -I: of every other name
const ENCODINGS: [&str; 4] = ["-O", "UTF-8", "-I", "UTF-8"];

pub fn contents(archive: &Path) -> Result<Contents> {
    let output = process::capture(
        Command::new("unzip")
            .args(ENCODINGS)
            .args(["-qq", "-l"])
            .arg(archive)
            .stderr(Stdio::piped()),
    )?;
    Ok(Contents::named(
        listing::lines(&output)
            .into_iter()
            .filter_map(|line| listing::split_fields(line, NAME_FIELD).pop())
            .map(<[u8]>::to_vec),
    ))
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    let mut command = Command::new("unzip");
    command
        .args(ENCODINGS)
        .args(["-qq", "-d"])
        .arg(request.directory()?);
    if let Overwrite::Replace = request.overwrite {
        command.arg("-o");
    }
    process::capture(command.arg(request.archive).stdin(Stdio::null()))?;
    Ok(())
}
