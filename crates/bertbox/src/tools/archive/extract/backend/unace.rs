use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::tools::archive::extract::backend::Request;
use crate::tools::archive::extract::config::Overwrite;
use crate::tools::archive::extract::listing::{self, Contents};
use crate::tools::archive::process;

const TABLE_HEADER: &[u8] = b"Date    Time     Packed      Size  Ratio  File";
const TABLE_FOOTER: &[u8] = b"listed:";
const NAME_FIELD: usize = 5;

pub fn contents(archive: &Path) -> Result<Contents> {
    let output = process::capture(Command::new("unace").arg("l").arg(archive))?;
    let lines: Vec<&[u8]> = listing::lines(&output)
        .into_iter()
        .map(<[u8]>::trim_ascii)
        .collect();
    let header = lines
        .iter()
        .position(|line| line.starts_with(TABLE_HEADER))
        .context("unable to parse unace output")?;
    let Some((footer, rows)) = lines[header + 1..].split_last() else {
        bail!("unable to parse unace output");
    };
    if !footer.starts_with(TABLE_FOOTER) {
        bail!("unable to parse unace output");
    }
    Ok(Contents::named(
        rows.iter()
            .filter_map(|row| listing::split_fields(row, NAME_FIELD).pop())
            .map(<[u8]>::to_vec),
    ))
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    request.without_password()?;
    let directory = request.directory()?;
    let mut command = Command::new("unace");
    command.arg("x");
    if let Overwrite::Replace = request.overwrite {
        command.arg("-o");
    }
    let mut target = OsString::from(directory);
    if !directory.as_os_str().as_encoded_bytes().ends_with(b"/") {
        target.push("/");
    }
    process::run(
        command
            .arg(request.archive)
            .arg(target)
            // unace only extracts into its working directory
            .current_dir(directory)
            .stdout(Stdio::null()),
    )
}
