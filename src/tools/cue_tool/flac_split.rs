use std::fs;
use std::path::{self, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::cue::split::{self, Options};
use crate::cue::{CueSheet, CueSheetError};
use crate::tools::cue_tool::cli::FlacSplit;

pub fn run(args: &FlacSplit) -> ExitCode {
    match tracks(args) {
        Ok(written) => {
            for path in written {
                println!("Wrote {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn tracks(args: &FlacSplit) -> Result<Vec<PathBuf>> {
    let cuefile = path::absolute(&args.cuefile)
        .with_context(|| format!("resolving {}", args.cuefile.display()))?;
    let outdir = cuefile
        .parent()
        .with_context(|| format!("{} has no parent", cuefile.display()))?;
    let data = fs::read(&cuefile).with_context(|| format!("reading {}", cuefile.display()))?;
    let sheet = match CueSheet::new(&data) {
        // The sheet is read as the text it decodes to, and left as it is.
        Err(CueSheetError::CharacterEncoding(text)) => CueSheet::new(text.as_bytes()),
        sheet => sheet,
    }
    .with_context(|| format!("parsing {}", cuefile.display()))?;
    let options = Options {
        lead_in: args.lead_in,
        gaps: args.gaps,
    };
    let written = split::tracks(&args.flac, &sheet, outdir, options)?;
    fs::remove_file(&args.flac).with_context(|| format!("removing {}", args.flac.display()))?;
    Ok(written)
}
