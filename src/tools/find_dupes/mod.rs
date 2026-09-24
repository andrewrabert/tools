mod action;
mod cli;
mod config;
mod dupes;
mod rank;
mod walk;

use std::collections::BTreeSet;
use std::process::ExitCode;

use anyhow::Result;

use crate::tools::Tool;
pub use crate::tools::find_dupes::cli::FindDupes;
use crate::tools::find_dupes::config::Config;

impl Tool for FindDupes {
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

fn run(args: FindDupes) -> Result<()> {
    let config = Config::try_from(args)?;
    let mut files = BTreeSet::new();
    for root in &config.roots {
        files.extend(walk::files_under(root)?);
    }
    let sets = dupes::find(files)?;
    action::run(&config.action, sets)
}
