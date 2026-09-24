//! The systemd user unit that hosts a daemon.
use std::env;
use std::fmt;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::dispatch;
use crate::tools::cdemu_tool::device::Images;

pub const TOOL_NAME: &str = "cdemu-tool";
const NAME_ENV: &str = "CDEMU_TOOL_NAME";
const RUNTIME_DIRECTORY_ENV: &str = "RUNTIME_DIRECTORY";

#[derive(Debug)]
pub enum UnitError {
    InvalidName(String),
}

impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnitError::InvalidName(name) => {
                write!(f, "{name:?} is not of the form {TOOL_NAME}@<digits>")
            }
        }
    }
}

impl std::error::Error for UnitError {}

/// A unit name of the form `cdemu-tool@<decimal digits>`, the digits being the load time in nanoseconds.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct UnitName(String);

impl FromStr for UnitName {
    type Err = UnitError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .strip_prefix(TOOL_NAME)
            .and_then(|rest| rest.strip_prefix('@'))
            .and_then(|digits| digits.parse::<u128>().ok())
            .ok_or_else(|| UnitError::InvalidName(value.to_owned()))?;
        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<String> for UnitName {
    type Error = UnitError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<UnitName> for String {
    fn from(name: UnitName) -> Self {
        name.0
    }
}

impl UnitName {
    fn generate() -> Result<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        Ok(Self(format!("{TOOL_NAME}@{nanos}")))
    }

    pub fn from_env() -> Result<Self> {
        let name = env::var(NAME_ENV).with_context(|| format!("{NAME_ENV} is not set"))?;
        Ok(name.parse()?)
    }

    pub fn stop(&self) -> Result<()> {
        let status = Command::new("systemctl")
            .arg("--user")
            .arg("stop")
            .arg(format!("{}.service", self.0))
            .status()
            .context("failed to run systemctl")?;
        if !status.success() {
            bail!("systemctl stop {}: {status}", self.0);
        }
        Ok(())
    }
}

pub fn start(images: &Images) -> Result<UnitName> {
    let name = UnitName::generate()?;
    let status = Command::new("systemd-run")
        .args(["--user", "--collect", "--quiet"])
        .arg(format!(
            "--property=RuntimeDirectory={TOOL_NAME}/{}",
            name.0
        ))
        .arg(format!("--unit={}", name.0))
        .arg(format!("--setenv={NAME_ENV}={}", name.0))
        .arg(format!(
            "--setenv={}={}",
            dispatch::ENV_NAME,
            dispatch::ENV_VALUE
        ))
        .args(["--", "dbus-run-session"])
        .arg(dispatch::program()?)
        .args([TOOL_NAME, "systemd-load"])
        .args(images.as_slice().iter().map(|image| image.as_path()))
        .status()
        .context("failed to run systemd-run")?;
    if !status.success() {
        bail!("systemd-run: {status}");
    }
    Ok(name)
}

pub fn runtime_directory() -> Result<PathBuf> {
    env::var_os(RUNTIME_DIRECTORY_ENV)
        .map(PathBuf::from)
        .with_context(|| format!("{RUNTIME_DIRECTORY_ENV} is not set"))
}
