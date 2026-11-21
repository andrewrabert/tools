use std::fs;
use std::io::ErrorKind;

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::install_target::InstallTarget;
use crate::tools::bertbox::packages::{files, paths};

pub fn read<T: DeserializeOwned>(name: &str) -> Result<Option<T>> {
    Ok(read_value(name)?.and_then(|value| serde_json::from_value(value).ok()))
}

pub fn read_value(name: &str) -> Result<Option<Value>> {
    let path = paths::record(name)?;
    match fs::read_to_string(&path) {
        Ok(text) => Ok(serde_json::from_str(&text).ok()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

pub fn write<T: Serialize>(name: &str, record: &T) -> Result<()> {
    let rendered = serde_json::to_string_pretty(record).context("rendering the record")?;
    files::write(&paths::record(name)?, format!("{rendered}\n").as_bytes())
}

pub fn target(name: &str) -> Result<Option<InstallTarget>> {
    Ok(read_value(name)?
        .and_then(|mut value| value.get_mut("target").map(Value::take))
        .and_then(|target| serde_json::from_value(target).ok()))
}
