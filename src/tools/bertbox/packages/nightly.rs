use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::install_target::InstallTarget;
use crate::tools::bertbox::packages::{download, files, paths, record};

#[derive(Serialize, Deserialize)]
struct Record {
    target: InstallTarget,
    etag: String,
    sha256: String,
}

pub fn install(
    name: &str,
    target: InstallTarget,
    repository: &str,
    artifact: &str,
    force: bool,
) -> Result<PathBuf> {
    let dest = paths::binary(name)?;
    let known = if force {
        None
    } else {
        valid_record(name, target, &dest)?
    };
    let url = format!("https://nightly.link/{repository}/workflows/ci/main/{artifact}");

    let client = download::client()?;
    let response = download::get(&client, &url)?;
    let etag = download::etag(&response)?;
    if known.is_some_and(|known| known.etag == etag) {
        println!("Already up to date");
        files::link_package(name)?;
        return Ok(dest);
    }
    let archive = download::receive(response)?;

    println!("Extracting...");
    let binary = download::sole_member(archive)?.data;
    files::write_binary(name, &binary)?;
    let installed = Record {
        target,
        etag,
        sha256: hex::encode(Sha256::digest(&binary)),
    };
    record::write(name, &installed)?;
    println!("Stored {} for sha256 {}", installed.etag, installed.sha256);
    files::link_package(name)?;
    Ok(dest)
}

fn valid_record(name: &str, target: InstallTarget, binary: &Path) -> Result<Option<Record>> {
    let Some(known) = record::read::<Record>(name)? else {
        return Ok(None);
    };
    if known.target != target {
        return Ok(None);
    }
    let Some(digest) = sha256(binary)? else {
        return Ok(None);
    };
    if known.sha256 != digest {
        println!("Installed binary does not match recorded hash");
        return Ok(None);
    }
    Ok(Some(known))
}

fn sha256(path: &Path) -> Result<Option<String>> {
    match fs::read(path) {
        Ok(data) => Ok(Some(hex::encode(Sha256::digest(data)))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}
