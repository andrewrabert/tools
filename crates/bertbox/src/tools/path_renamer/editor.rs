use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::process::Command;

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

const EDITOR: &str = "nvim";

pub fn edit(names: &[OsString]) -> Result<Vec<OsString>> {
    let file = NamedTempFile::new().context("creating a temporary file")?;
    let lines: Vec<&[u8]> = names.iter().map(|name| name.as_bytes()).collect();
    fs::write(file.path(), lines.join(&b'\n'))
        .with_context(|| format!("writing {}", file.path().display()))?;

    let status = Command::new(EDITOR)
        .arg(file.path())
        .status()
        .with_context(|| format!("running {EDITOR}"))?;
    if !status.success() {
        bail!("{EDITOR} failed: {status}");
    }

    let edited =
        fs::read(file.path()).with_context(|| format!("reading {}", file.path().display()))?;
    let mut targets = Vec::new();
    for line in edited.split_inclusive(|&byte| byte == b'\n') {
        let name = line.trim_ascii();
        if name.is_empty() {
            bail!("empty target name on line {}", targets.len() + 1);
        }
        targets.push(OsString::from_vec(name.to_vec()));
    }
    Ok(targets)
}
