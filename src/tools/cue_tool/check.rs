use std::fs;
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::cue::{CueSheet, CueSheetError};
use crate::source::Source;
use crate::tools::cue_tool::cli::Check;

/// Checks each cue sheet, printing each error to stderr. Unless `parse_only`, each file a sheet
/// lists must exist.
pub fn run(args: &Check) -> ExitCode {
    let sources = match super::sources(&args.input) {
        Ok(sources) => sources,
        Err(error) => {
            eprintln!("Error: {error:#}");
            return ExitCode::FAILURE;
        }
    };
    let mut failed = false;
    for source in &sources {
        match validate(source, args.fix_encoding) {
            Ok((errors, sheet)) => {
                for error in &errors {
                    eprintln!("Error: {}: {error}", super::name(source));
                }
                failed |= !errors.is_empty();
                if !args.parse_only
                    && let Some(sheet) = sheet
                {
                    failed |= !files_exist(source, &sheet);
                }
            }
            Err(error) => {
                eprintln!("Error: {error:#}");
                failed = true;
            }
        }
    }
    if failed && !args.exit_zero {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// The sheet's errors, and the sheet when it parses. After a character encoding error, the sheet
/// converted to UTF-8 is checked too. With `fix_encoding`, the converted sheet replaces the file
/// and the encoding error is not reported.
fn validate(source: &Source, fix_encoding: bool) -> Result<(Vec<CueSheetError>, Option<CueSheet>)> {
    let data = super::read(source)?;
    let mut errors = Vec::new();
    let sheet = match CueSheet::new(&data) {
        Err(CueSheetError::CharacterEncoding(text)) => {
            let sheet = CueSheet::new(text.as_bytes());
            match source {
                Source::File(path) if fix_encoding => {
                    fs::write(path, text.as_bytes())
                        .with_context(|| format!("writing {}", path.display()))?;
                    eprintln!("Converted {} to UTF-8", path.display());
                }
                Source::File(_) | Source::Stdin => {
                    errors.push(CueSheetError::CharacterEncoding(text));
                }
            }
            sheet
        }
        sheet => sheet,
    };
    let sheet = match sheet {
        Ok(sheet) => Some(sheet),
        Err(error) => {
            errors.push(error);
            None
        }
    };
    Ok((errors, sheet))
}

/// Whether each file the sheet lists exists, printing each missing file to stderr.
fn files_exist(source: &Source, sheet: &CueSheet) -> bool {
    let files = match super::listed_files(source, sheet) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("Error: {error:#}");
            return false;
        }
    };
    let mut exist = true;
    for file in files.iter().filter(|file| !file.is_file()) {
        eprintln!(
            "Error: {}: missing file {}",
            super::name(source),
            file.display()
        );
        exist = false;
    }
    exist
}
