//! The D-Bus client for one cdemu-daemon process.
use std::collections::HashMap;
use std::fmt;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use zbus::Message;
use zbus::blocking::Connection;
use zbus::zvariant::{DynamicType, Value};

use crate::tools::cdemu_tool::device::{Images, SgDevice, SrDevice};

const SERVICE: &str = "net.sf.cdemu.CDEmuDaemon";
const OBJECT_PATH: &str = "/Daemon";
const DEVICE: i32 = 0;
const SUPPORTED_INTERFACE_VERSION: (i32, i32) = (7, 0);
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub struct Mapping {
    pub sr: SrDevice,
    pub sg: SgDevice,
}

#[derive(Debug)]
pub enum DaemonError {
    UnsupportedInterface {
        major: i32,
        minor: i32,
        required: (i32, i32),
    },
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DaemonError::UnsupportedInterface {
                major,
                minor,
                required: (required_major, required_minor),
            } => write!(
                f,
                "cdemu daemon interface version {major}.{minor} detected, but version {required_major}.{required_minor} is required"
            ),
        }
    }
}

impl std::error::Error for DaemonError {}

pub struct Daemon {
    process: Child,
    connection: Connection,
}

impl Daemon {
    pub fn spawn() -> Result<Self> {
        let process = Command::new("cdemu-daemon")
            .spawn()
            .context("failed to run cdemu-daemon")?;
        let connection = Connection::session().context("failed to connect to the session bus")?;
        let daemon = Self {
            process,
            connection,
        };
        daemon.check_interface_version()?;
        Ok(daemon)
    }

    fn check_interface_version(&self) -> Result<()> {
        let reply = self.call("GetDaemonInterfaceVersion2", &())?;
        let (major, minor): (i32, i32) = reply.body().deserialize()?;
        let (supported_major, supported_minor) = SUPPORTED_INTERFACE_VERSION;
        if major != supported_major || minor < supported_minor {
            return Err(DaemonError::UnsupportedInterface {
                major,
                minor,
                required: SUPPORTED_INTERFACE_VERSION,
            }
            .into());
        }
        Ok(())
    }

    pub fn wait_for_mapping(&self) -> Result<Mapping> {
        loop {
            let reply = self.call("DeviceGetMapping", &(DEVICE,))?;
            let (sr, sg): (String, String) = reply.body().deserialize()?;
            if !sr.is_empty() {
                return Ok(Mapping {
                    sr: sr.into(),
                    sg: sg.into(),
                });
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    pub fn load(&self, images: &Images) -> Result<()> {
        let files = images
            .as_slice()
            .iter()
            .map(|image| -> Result<String> {
                image
                    .as_path()
                    .to_path_buf()
                    .into_os_string()
                    .into_string()
                    .map_err(|file| anyhow!("{file:?} is not UTF-8"))
            })
            .collect::<Result<Vec<String>>>()?;
        let parameters: HashMap<&str, Value<'_>> = HashMap::new();
        self.call("DeviceLoad", &(DEVICE, files, parameters))?;
        Ok(())
    }

    pub fn wait_until_loaded(&self) -> Result<()> {
        loop {
            let reply = self.call("DeviceGetStatus", &(DEVICE,))?;
            let (loaded, _filenames): (bool, Vec<String>) = reply.body().deserialize()?;
            if loaded {
                return Ok(());
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    pub fn wait(mut self) -> Result<()> {
        self.process
            .wait()
            .context("failed to wait for cdemu-daemon")?;
        Ok(())
    }

    fn call<B>(&self, method: &str, body: &B) -> Result<Message>
    where
        B: Serialize + DynamicType,
    {
        self.connection
            .call_method(Some(SERVICE), OBJECT_PATH, Some(SERVICE), method, body)
            .with_context(|| format!("cdemu daemon {method} failed"))
    }
}
