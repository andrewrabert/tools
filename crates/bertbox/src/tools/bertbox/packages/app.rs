use std::fs;

use anyhow::{Context, Result, bail};

use crate::install_target::InstallTarget;
use crate::tools::bertbox::packages::nightly::ArtifactDownload;
use crate::tools::bertbox::packages::{
    Installation, NightlyArtifact, PackageName, Version, download, flatpak, mac_app,
};

#[derive(Clone, Copy)]
pub enum AppBundle {
    Flatpak,
    MacApp { app: &'static str },
}

impl AppBundle {
    fn extension(&self) -> &'static str {
        match self {
            AppBundle::Flatpak => "flatpak",
            AppBundle::MacApp { .. } => "dmg",
        }
    }
}

pub fn deploy(
    name: PackageName,
    target: InstallTarget,
    artifact: &NightlyArtifact,
    bundle: AppBundle,
    force: bool,
) -> Result<()> {
    let known = if force {
        None
    } else {
        Installation::load(name)?.filter(|known| known.target == target)
    };

    let ArtifactDownload { etag, body } = artifact.download()?;
    let version = Version::Etag(etag.clone());
    if known.is_some_and(|known| known.version == version) {
        println!("Already up to date");
        return Ok(());
    }
    let archive = download::receive(body)?;

    println!("Extracting...");
    let member = download::sole_member(archive)?;
    let extension = bundle.extension();
    let file_name = member
        .name
        .rsplit('/')
        .next()
        .unwrap_or(&member.name)
        .to_owned();
    if !file_name.ends_with(&format!(".{extension}")) {
        bail!("expected a .{extension}, got {file_name}");
    }
    let staging = tempfile::tempdir().context("creating a temporary directory")?;
    let path = staging.path().join(&file_name);
    fs::write(&path, &member.data).with_context(|| format!("writing {}", path.display()))?;

    match bundle {
        AppBundle::Flatpak => flatpak::install_bundle(&path)?,
        AppBundle::MacApp { app } => mac_app::install_bundle(&path, app)?,
    }
    Installation {
        target,
        version,
        sha256: None,
    }
    .store(name)?;
    println!("Stored etag {etag}");
    Ok(())
}
