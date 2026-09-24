use std::fs;
use std::process::Command;

use anyhow::{Context, Result, bail};

const MOUNTS: &str = "/proc/mounts";

pub enum RootFilesystem {
    AlreadyWritable,
    Remounted,
}

impl RootFilesystem {
    pub fn make_writable() -> Result<Self> {
        let mounts = fs::read_to_string(MOUNTS).with_context(|| format!("reading {MOUNTS}"))?;
        let read_only = mounts.lines().any(|line| {
            let mut fields = line.split_ascii_whitespace().skip(1);
            fields.next() == Some("/")
                && fields
                    .nth(1)
                    .is_some_and(|options| options.split(',').any(|option| option == "ro"))
        });
        if !read_only {
            return Ok(RootFilesystem::AlreadyWritable);
        }
        let status = Command::new("rw").status().context("running rw")?;
        if !status.success() {
            bail!("rw failed: {status}");
        }
        Ok(RootFilesystem::Remounted)
    }
}

impl Drop for RootFilesystem {
    fn drop(&mut self) {
        if let RootFilesystem::Remounted = self {
            match Command::new("ro").status() {
                Ok(status) if status.success() => {}
                Ok(status) => eprintln!("ro failed: {status}"),
                Err(e) => eprintln!("running ro: {e}"),
            }
        }
    }
}
