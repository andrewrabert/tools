use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::LazyLock;

use anyhow::Result;
use clap::Args as ClapArgs;
use ignore::WalkBuilder;
use regex::Regex;

use crate::tools::Tool;

static CONFLICT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.sync-conflict-\d{8}-\d{6}").expect("valid regex"));

#[derive(ClapArgs)]
#[command(about = "List Syncthing conflict files")]
pub struct SyncthingConflicts {
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
}

impl Tool for SyncthingConflicts {
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

fn run(args: SyncthingConflicts) -> Result<()> {
    for entry in WalkBuilder::new(&args.path).standard_filters(false).build() {
        let entry = entry?;
        let is_file = entry.file_type().is_some_and(|kind| kind.is_file());
        if is_file && CONFLICT.is_match(&entry.file_name().to_string_lossy()) {
            println!("{}", entry.path().display());
        }
    }
    Ok(())
}
