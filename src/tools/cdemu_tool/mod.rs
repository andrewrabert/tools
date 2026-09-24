//! CLI dispatch and the in-process API (`devices`, `load`, `remove`) for other tools.
mod cli;
mod daemon;
mod device;
mod store;
mod unit;

use std::env;
use std::process::ExitCode;
use std::thread;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::tools::Tool;
pub use crate::tools::cdemu_tool::cli::CdemuTool;
use crate::tools::cdemu_tool::cli::Command;
use crate::tools::cdemu_tool::daemon::{Daemon, POLL_INTERVAL};
use crate::tools::cdemu_tool::device::BusAddress;
pub use crate::tools::cdemu_tool::device::{Device, Devices, ImagePath, Images, SrDevice};
use crate::tools::cdemu_tool::unit::UnitName;

impl Tool for CdemuTool {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: CdemuTool) -> Result<()> {
    match args.command {
        Command::Load { files } => print_json(&load(&Images::from_paths(files)?)?),
        Command::Remove { device } => remove(&device),
        Command::Status { json } => status(json),
        Command::SystemdLoad { files } => systemd_load(Images::from_paths(files)?),
    }
}

pub fn devices() -> Result<Devices> {
    store::read(&store::runtime_dir()?)
}

pub fn load(images: &Images) -> Result<Device> {
    let runtime_dir = store::runtime_dir()?;
    let name = unit::start(images)?;
    loop {
        if let Some(device) = store::read(&runtime_dir)?.find_by_name(&name) {
            return Ok(device.clone());
        }
        thread::sleep(POLL_INTERVAL);
    }
}

pub fn remove(dev_sr: &SrDevice) -> Result<()> {
    let devices = devices()?;
    let device = devices.get(dev_sr).context("device not found")?;
    device.name().stop()
}

fn status(json: bool) -> Result<()> {
    let devices = devices()?;
    if json {
        return print_json(&devices);
    }
    for device in devices.iter() {
        println!("{}", device.dev_sr());
        for filename in device.filenames().as_slice() {
            println!("  {}", filename.as_path().display());
        }
    }
    Ok(())
}

fn systemd_load(images: Images) -> Result<()> {
    let name = UnitName::from_env()?;
    let bus_address: BusAddress = env::var("DBUS_SESSION_BUS_ADDRESS")
        .context("DBUS_SESSION_BUS_ADDRESS is not set")?
        .into();
    let runtime_directory = unit::runtime_directory()?;
    let daemon = Daemon::spawn()?;
    let mapping = daemon.wait_for_mapping()?;
    daemon.load(&images)?;
    daemon.wait_until_loaded()?;
    let device = Device::new(bus_address, mapping.sr, mapping.sg, images, name);
    store::write(&device, &runtime_directory)?;
    daemon.wait()
}

fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
