use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::install_target::{InstallTarget, PackageManager, System};
use crate::tools::bertbox::packages::nightly::ArtifactDownload;
use crate::tools::bertbox::packages::{
    NightlyArtifact, PackageName, Version, download, files, flatpak, mac_app, paths,
};

#[derive(Serialize, Deserialize)]
struct FlatpakManifest {
    version: Version,
}

#[derive(Serialize, Deserialize)]
struct MacosManifest {
    version: Version,
}

fn manifest_path(name: PackageName) -> Result<PathBuf> {
    Ok(paths::system_dir()?
        .join(name.as_str())
        .join("version.json"))
}

fn load<T: DeserializeOwned>(name: PackageName) -> Result<Option<T>> {
    let path = manifest_path(name)?;
    match fs::read_to_string(&path) {
        Ok(text) => Ok(serde_json::from_str(&text).ok()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

fn store(name: PackageName, manifest: &impl Serialize) -> Result<()> {
    let rendered = serde_json::to_string_pretty(manifest).context("rendering the manifest")?;
    files::write(&manifest_path(name)?, format!("{rendered}\n").as_bytes())
}

pub fn installed_target(name: PackageName, host: InstallTarget) -> Result<Option<InstallTarget>> {
    let InstallTarget(system, arch, _) = host;
    Ok(match system {
        System::Linux => load::<FlatpakManifest>(name)?
            .map(|_| InstallTarget(system, arch, Some(PackageManager::Flatpak))),
        System::Macos => load::<MacosManifest>(name)?.map(|_| host),
        System::Termux => None,
    })
}

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
    artifact: &NightlyArtifact,
    bundle: AppBundle,
    force: bool,
) -> Result<()> {
    let known = if force {
        None
    } else {
        match bundle {
            AppBundle::Flatpak => load::<FlatpakManifest>(name)?.map(|known| known.version),
            AppBundle::MacApp { .. } => load::<MacosManifest>(name)?.map(|known| known.version),
        }
    };

    let ArtifactDownload { etag, body } = artifact.download()?;
    let version = Version::Etag(etag.clone());
    if known.is_some_and(|known| known == version) {
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
        AppBundle::Flatpak => {
            flatpak::install_bundle(&path)?;
            store(name, &FlatpakManifest { version })?;
        }
        AppBundle::MacApp { app } => {
            mac_app::install_bundle(&path, app)?;
            store(name, &MacosManifest { version })?;
        }
    }
    println!("Stored etag {etag}");
    Ok(())
}
