use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use clap::Args as ClapArgs;
use serde::Serialize;

use crate::tools::Tool;
use crate::tools::file_hash_recorder::{scan, walk};

#[derive(ClapArgs)]
#[command(about = "Print the hashes of a file, or of every file under a directory, as JSON")]
pub struct Multihash {
    #[arg(long)]
    verbose: bool,
    #[arg(short, long, help = "output compact json")]
    compact: bool,
    #[arg(value_name = "PATH")]
    path: PathBuf,
}

impl Tool for Multihash {
    fn run(self) -> ExitCode {
        match run(&self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: &Multihash) -> Result<()> {
    let root = &args.path;
    if root.is_file() {
        announce(root, args.verbose);
        return print_json(&scan::fingerprint(root)?, args.compact);
    }
    let mut fingerprints = BTreeMap::new();
    for path in walk::files_under(root)? {
        announce(&path, args.verbose);
        let name = scan::relative_name(&path, root)?;
        fingerprints.insert(name, scan::fingerprint(&path)?);
    }
    print_json(&fingerprints, args.compact)
}

fn announce(path: &Path, verbose: bool) {
    if verbose {
        println!("Processing {}", path.display());
    }
}

fn print_json(value: &impl Serialize, compact: bool) -> Result<()> {
    let text = if compact {
        serde_json::to_string(value)?
    } else {
        serde_json::to_string_pretty(value)?
    };
    println!("{text}");
    Ok(())
}
