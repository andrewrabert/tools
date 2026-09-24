mod cli;
mod editor;
mod siblings;

use std::ffi::OsString;
use std::io::Read;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::source::Source;
use crate::tools::Tool;
pub use crate::tools::path_renamer::cli::PathRenamer;
use crate::tools::path_renamer::siblings::{Collisions, Siblings};

impl Tool for PathRenamer {
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

fn run(args: PathRenamer) -> Result<()> {
    let collisions = if args.allow_duplicates {
        Collisions::Suffix
    } else {
        Collisions::Reject
    };
    let siblings = Siblings::resolve(&listed(&args.data)?)?;
    let targets = editor::edit(siblings.names())?;
    for rename in siblings.renames(&targets, &collisions)? {
        rename.apply(&collisions)?;
    }
    Ok(())
}

fn listed(source: &Source) -> Result<Vec<PathBuf>> {
    let mut data = Vec::new();
    source
        .open()?
        .read_to_end(&mut data)
        .context("reading paths")?;
    let paths = data
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| PathBuf::from(OsString::from_vec(line.to_vec())))
        .collect();
    Ok(paths)
}
