use std::fs;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::tools::Tool;

const MEMINFO: &str = "/proc/meminfo";
const FIELDS: [&str; 2] = ["Dirty:", "Writeback:"];
const UNITS: [&str; 4] = ["K", "M", "G", "T"];

#[derive(ClapArgs)]
#[command(about = "Print how much data is waiting to be written to disk")]
pub struct PendingSync {}

impl Tool for PendingSync {
    fn run(self) -> ExitCode {
        match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run() -> Result<()> {
    let meminfo = fs::read_to_string(MEMINFO).with_context(|| format!("reading {MEMINFO}"))?;
    let mut kibibytes = 0;
    for line in meminfo.lines() {
        let mut fields = line.split_whitespace();
        if !fields.next().is_some_and(|name| FIELDS.contains(&name)) {
            continue;
        }
        let value = fields
            .next()
            .with_context(|| format!("{MEMINFO} line has no value: {line:?}"))?;
        kibibytes += value
            .parse::<u64>()
            .with_context(|| format!("parsing {MEMINFO} line {line:?}"))?;
    }
    println!("{}", human_size(kibibytes));
    Ok(())
}

fn human_size(kibibytes: u64) -> String {
    let mut size = kibibytes as f64;
    let mut unit = 0;
    while size > 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1}{}", UNITS[unit])
}
