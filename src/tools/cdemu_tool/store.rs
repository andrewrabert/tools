//! Reading and writing the device records at `$XDG_RUNTIME_DIR/cdemu-tool/<unit>/info.json`.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

use crate::dirs;
use crate::tools::cdemu_tool::device::{Device, Devices};
use crate::tools::cdemu_tool::unit::TOOL_NAME;

const INFO_FILE: &str = "info.json";

pub fn runtime_dir() -> Result<PathBuf> {
    dirs::runtime()
}

pub fn read(runtime_dir: &Path) -> Result<Devices> {
    let parent = runtime_dir.join(TOOL_NAME);
    let entries = match fs::read_dir(&parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Devices::new(Vec::new())?);
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", parent.display()));
        }
    };
    let mut records: Vec<(PathBuf, Device)> = Vec::new();
    for entry in entries {
        let path = entry
            .with_context(|| format!("failed to read {}", parent.display()))?
            .path()
            .join(INFO_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read {}", path.display()));
            }
        };
        let device = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        records.push((path, device));
    }
    Ok(Devices::new(records)?)
}

pub fn write(device: &Device, directory: &Path) -> Result<()> {
    let path = directory.join(INFO_FILE);
    let mut file = NamedTempFile::new_in(directory)
        .with_context(|| format!("failed to create a file in {}", directory.display()))?;
    serde_json::to_writer(&mut file, device)?;
    file.persist(&path)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}
