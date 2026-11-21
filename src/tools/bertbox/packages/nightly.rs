use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::install_target::InstallTarget;
use crate::tools::bertbox::packages::{
    GitHubRepository, Installation, PackageName, Version, download, files, paths,
};

#[derive(Clone, Copy)]
pub struct NightlyArtifact {
    pub repository: GitHubRepository,
    pub workflow: &'static str,
    pub artifact: &'static str,
}

impl NightlyArtifact {
    pub fn url(&self) -> String {
        format!(
            "https://nightly.link/{}/workflows/{}/main/{}",
            self.repository, self.workflow, self.artifact
        )
    }
}

pub fn deploy(
    name: PackageName,
    target: InstallTarget,
    artifact: &NightlyArtifact,
    force: bool,
) -> Result<PathBuf> {
    let dest = paths::binary(name)?;
    let known = if force {
        None
    } else {
        valid_installation(name, target, &dest)?
    };

    let client = download::client()?;
    let response = download::get(&client, &artifact.url())?;
    let etag = Version::Etag(download::etag(&response)?);
    if known.is_some_and(|known| known.version == etag) {
        println!("Already up to date");
        files::link_package(name)?;
        return Ok(dest);
    }
    let archive = download::receive(response)?;

    println!("Extracting...");
    let binary = download::sole_member(archive)?.data;
    files::write_binary(name, &binary)?;
    let sha256 = hex::encode(Sha256::digest(&binary));
    let installation = Installation {
        target,
        version: etag,
        sha256: Some(sha256.clone()),
    };
    installation.store(name)?;
    if let Version::Etag(etag) = &installation.version {
        println!("Stored {etag} for sha256 {sha256}");
    }
    files::link_package(name)?;
    Ok(dest)
}

fn valid_installation(
    name: PackageName,
    target: InstallTarget,
    binary: &Path,
) -> Result<Option<Installation>> {
    let Some(known) = Installation::load(name)? else {
        return Ok(None);
    };
    if known.target != target {
        return Ok(None);
    }
    let Some(digest) = sha256(binary)? else {
        return Ok(None);
    };
    if known.sha256.as_deref() != Some(digest.as_str()) {
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
