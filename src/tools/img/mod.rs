mod cache;
mod cli;
mod compare;
mod convert;
mod encode;
mod exif;
mod fixcolorspace;
mod mime;
mod mozjpeg;
mod optim;
mod optimize;
mod probe;
mod process;
mod svg;
mod temp;
mod walk;

use std::num::NonZeroUsize;
use std::path::Path;
use std::process::ExitCode;
use std::thread;

use anyhow::{Context, Result, bail};

use crate::tools::Tool;
use crate::tools::img::cli::MetadataCommand;
pub use crate::tools::img::cli::{Compare, Convert, Fixcolorspace, Metadata, Optim};

fn report(result: Result<ExitCode>) -> ExitCode {
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        }
    }
}

impl Tool for Compare {
    fn run(self) -> ExitCode {
        report(compare::run(self))
    }
}

impl Tool for Convert {
    fn run(self) -> ExitCode {
        report(convert::run(self))
    }
}

impl Tool for Fixcolorspace {
    fn run(self) -> ExitCode {
        report(fixcolorspace::run(self))
    }
}

impl Tool for Metadata {
    fn run(self) -> ExitCode {
        report(metadata(self))
    }
}

impl Tool for Optim {
    fn run(self) -> ExitCode {
        report(optim::run(self))
    }
}

fn metadata(args: Metadata) -> Result<ExitCode> {
    match args.command {
        MetadataCommand::Copy { source, target } => {
            require_file(&source)?;
            require_file(&target)?;
            exif::copy_metadata(&source, &target)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// The worker count for `--num-concurrent`, where 0 means one per cpu.
pub(crate) fn jobs(num_procs: usize) -> Result<NonZeroUsize> {
    match NonZeroUsize::new(num_procs) {
        Some(jobs) => Ok(jobs),
        None => thread::available_parallelism().context("counting cpus"),
    }
}

pub(crate) fn require_file(path: &Path) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else if path.is_dir() {
        bail!("path must be a file: {}", path.display())
    } else {
        bail!("invalid path: {}", path.display())
    }
}
