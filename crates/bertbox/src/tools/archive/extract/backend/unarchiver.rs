use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::tools::archive::extract::backend::Request;
use crate::tools::archive::extract::config::Overwrite;
use crate::tools::archive::extract::listing::{Contents, Entry};
use crate::tools::archive::process;

const RESOURCE_FORK_SUFFIX: &str = ".rsrc";

/// The output of `lsar -json`.
#[derive(Deserialize)]
struct Listing {
    #[serde(rename = "lsarContents")]
    contents: Option<Vec<Item>>,
    #[serde(rename = "lsarProperties", default)]
    properties: Properties,
}

#[derive(Deserialize)]
struct Item {
    #[serde(rename = "XADFileName")]
    name: String,
    #[serde(rename = "XADIsResourceFork", default)]
    is_resource_fork: i64,
}

#[derive(Default, Deserialize)]
struct Properties {
    #[serde(rename = "XADVolumes")]
    volumes: Option<Vec<PathBuf>>,
}

fn lsar(archive: &Path) -> Result<Listing> {
    let output = process::capture(Command::new("lsar").args(["-json", "--"]).arg(archive))?;
    serde_json::from_slice(&output).context("parsing the output of lsar")
}

pub fn contents(archive: &Path) -> Result<Contents> {
    let items = lsar(archive)?
        .contents
        .context("lsar reported no contents")?;
    let mut contents = Contents::default();
    for item in items {
        let mut name = item.name;
        if item.is_resource_fork != 0 {
            name.push_str(RESOURCE_FORK_SUFFIX);
        }
        if contents
            .insert(name.clone().into_bytes(), Entry::default())
            .is_some()
        {
            bail!("lsar listed {name} twice");
        }
    }
    Ok(contents)
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    let mut command = Command::new("unar");
    command
        .args(["-quiet", "-output-directory"])
        .arg(request.directory()?);
    if let Overwrite::Replace = request.overwrite {
        command.arg("-force-overwrite");
    }
    process::capture(command.arg("--").arg(request.archive).stdin(Stdio::null()))?;
    Ok(())
}

pub fn volumes(archive: &Path) -> Result<Vec<PathBuf>> {
    lsar(archive)?
        .properties
        .volumes
        .context("lsar reported no volumes")
}
