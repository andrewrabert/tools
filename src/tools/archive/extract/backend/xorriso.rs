use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;

use crate::tools::archive::extract::backend::Request;
use crate::tools::archive::extract::listing::{self, Contents};
use crate::tools::archive::process;

pub fn contents(archive: &Path) -> Result<Contents> {
    let output = process::capture(
        Command::new("xorriso")
            .args(["-osirrox", "on", "-indev"])
            .arg(archive)
            .args(["-abort_on", "WARNING", "-find", "/", "-exec", "lsdl"])
            .stderr(Stdio::null()),
    )?;

    let mut directories = BTreeSet::new();
    let mut files = BTreeSet::new();
    for line in listing::lines(&output) {
        let Some(name) = quoted_name(line) else {
            continue;
        };
        if line.starts_with(b"d") {
            directories.insert(name);
        } else {
            files.insert(name);
        }
    }

    let mut names = files.clone();
    for directory in directories {
        let mut prefix = directory.clone();
        prefix.push(b'/');
        if !files.iter().any(|file| file.starts_with(&prefix)) {
            names.insert(prefix);
        }
    }
    Ok(Contents::named(names))
}

fn quoted_name(line: &[u8]) -> Option<Vec<u8>> {
    let open = line.iter().position(|byte| *byte == b'\'')?;
    let quoted = line[open + 1..].strip_suffix(b"'")?;
    let name = quoted.trim_ascii();
    let start = name.iter().position(|byte| *byte != b'/')?;
    Some(name[start..].to_vec())
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    request.keeping_existing()?;
    process::run(
        Command::new("xorriso")
            .args(["-osirrox", "on", "-indev"])
            .arg(request.archive)
            .args(["-chmod_r", "u+rw", "/", "--", "-extract", "/"])
            .arg(request.directory()?)
            .arg("-rollback_end")
            .stderr(Stdio::null()),
    )
}
