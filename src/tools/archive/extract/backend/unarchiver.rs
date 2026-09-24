use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::tools::archive::extract::backend::Request;
use crate::tools::archive::extract::config::Overwrite;
use crate::tools::archive::extract::listing::{Contents, Entry};
use crate::tools::archive::process;

const RESOURCE_FORK_SUFFIX: &str = ".rsrc";

fn lsar(archive: &Path) -> Result<Value> {
    let output = process::capture(Command::new("lsar").args(["-json", "--"]).arg(archive))?;
    serde_json::from_slice(&output).context("parsing the output of lsar")
}

pub fn contents(archive: &Path) -> Result<Contents> {
    let info = lsar(archive)?;
    let items = info["lsarContents"]
        .as_array()
        .context("lsar reported no contents")?;
    let mut contents = Contents::default();
    for item in items {
        let mut name = item["XADFileName"]
            .as_str()
            .context("lsar reported an entry without a name")?
            .to_owned();
        if item["XADIsResourceFork"].as_i64().unwrap_or(0) != 0 {
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
    let info = lsar(archive)?;
    info["lsarProperties"]["XADVolumes"]
        .as_array()
        .context("lsar reported no volumes")?
        .iter()
        .map(|volume| {
            volume
                .as_str()
                .map(PathBuf::from)
                .context("lsar reported a volume that is not a string")
        })
        .collect()
}
