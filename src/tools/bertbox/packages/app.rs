use std::fs;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::install_target::InstallTarget;
use crate::tools::bertbox::packages::{download, flatpak, mac_app, record};

pub struct Source<'a> {
    pub repository: &'a str,
    pub workflow: &'a str,
    pub artifact: &'a str,
}

pub enum Installer<'a> {
    Flatpak,
    MacApp { app: &'a str },
}

impl Installer<'_> {
    fn extension(&self) -> &'static str {
        match self {
            Installer::Flatpak => "flatpak",
            Installer::MacApp { .. } => "dmg",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Record {
    target: InstallTarget,
    etag: String,
}

pub fn install(
    name: &str,
    target: InstallTarget,
    source: &Source<'_>,
    installer: Installer<'_>,
    force: bool,
) -> Result<()> {
    let known = if force {
        None
    } else {
        record::read::<Record>(name)?.filter(|known| known.target == target)
    };
    let url = format!(
        "https://nightly.link/{}/workflows/{}/main/{}",
        source.repository, source.workflow, source.artifact
    );

    let client = download::client()?;
    let response = download::get(&client, &url)?;
    let etag = download::etag(&response)?;
    if known.is_some_and(|known| known.etag == etag) {
        println!("Already up to date");
        return Ok(());
    }
    let archive = download::receive(response)?;

    println!("Extracting...");
    let member = download::sole_member(archive)?;
    let extension = installer.extension();
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

    match installer {
        Installer::Flatpak => flatpak::install(&path)?,
        Installer::MacApp { app } => mac_app::install(&path, app)?,
    }
    record::write(
        name,
        &Record {
            target,
            etag: etag.clone(),
        },
    )?;
    println!("Stored etag {etag}");
    Ok(())
}
