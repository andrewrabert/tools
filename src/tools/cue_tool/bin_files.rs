//! The file-merge and file-split subcommands.
use std::fs;
use std::process::ExitCode;

use anyhow::{Context, Result, ensure};

use crate::cue::bin::{self, LeadIn, Operation, Replacement};
use crate::tools::cue_tool::cli::BinFiles;

pub fn run(args: &BinFiles, operation: Operation) -> ExitCode {
    match each(args, operation) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Runs every cue sheet, past one that fails, which makes the exit code a failure.
fn each(args: &BinFiles, operation: Operation) -> Result<ExitCode> {
    ensure!(
        args.name.is_none() || args.cuefiles.len() == 1,
        "--name names the files of one CUEFILE, but {} were given",
        args.cuefiles.len()
    );
    ensure!(
        !(args.in_place && matches!(operation, Operation::Split(LeadIn::Discard))),
        "--lead-in discard leaves sectors out, so it cannot be used with --in-place"
    );
    if let Some(outdir) = &args.outdir {
        fs::create_dir_all(outdir).with_context(|| format!("creating {}", outdir.display()))?;
    }
    let mut code = ExitCode::SUCCESS;
    for cuefile in &args.cuefiles {
        let result = if args.in_place {
            bin::replace(cuefile, operation).map(|replacement| {
                let cuefile = cuefile.display();
                match (operation, replacement) {
                    (Operation::Merge, Replacement::Replaced { old, .. }) => {
                        println!("Merged {cuefile}: {old} files into 1");
                    }
                    (Operation::Split(_), Replacement::Replaced { new, .. }) => {
                        println!("Split {cuefile}: 1 file into {new}");
                    }
                    (Operation::Merge, Replacement::Unneeded) => {
                        println!("Skipped {cuefile}: already one file");
                    }
                    (Operation::Split(_), Replacement::Unneeded) => {
                        println!("Skipped {cuefile}: already one file per track");
                    }
                }
            })
        } else {
            bin::add(
                cuefile,
                args.outdir.as_deref(),
                args.name.as_deref(),
                operation,
            )
            .map(|written| {
                for path in written.bins.iter().chain([&written.cue]) {
                    println!("Wrote {}", path.display());
                }
            })
        };
        if let Err(error) = result {
            eprintln!("Error: {error:#}");
            code = ExitCode::FAILURE;
        }
    }
    Ok(code)
}
