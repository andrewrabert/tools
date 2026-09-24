//! CLI dispatch and what the subcommands share.
mod check;
mod cli;
mod files;
mod fmt;
mod json;

use std::collections::BTreeSet;
use std::env;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

use crate::cue::{self, CueSheet};
use crate::source::Source;
use crate::tools::Tool;
pub use crate::tools::cue_tool::cli::CueTool;
use crate::tools::cue_tool::cli::{Command, Input};
use crate::walk;

impl Tool for CueTool {
    fn run(self) -> ExitCode {
        match self.command {
            Command::Check(args) => check::run(&args),
            Command::Fmt(args) => fmt::run(&args),
            Command::Json(args) => json::run(&args),
            Command::Files(args) => files::run(&args),
        }
    }
}

/// The cue sheets `input` names: stdin without a path, the path, or with `recursive` the cue
/// sheets under the path, sorted. Stdin must not be a terminal.
fn sources(input: &Input) -> Result<Vec<Source>> {
    match (&input.path, input.recursive) {
        (None, _) if io::stdin().is_terminal() => bail!("expected PATH or piped stdin"),
        (None, _) => Ok(vec![Source::Stdin]),
        (Some(path), false) => Ok(vec![Source::File(path.clone())]),
        (Some(root), true) => {
            let mut sheets = BTreeSet::new();
            for entry in walk::entries(root) {
                let entry = entry?;
                let is_cue = entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case(cue::EXTENSION));
                if entry.file_type().is_file() && is_cue {
                    sheets.insert(entry.into_path());
                }
            }
            Ok(sheets.into_iter().map(Source::File).collect())
        }
    }
}

/// Runs `each` on every cue sheet `input` names, printing each error to stderr. Fails when any
/// sheet fails.
fn for_each(input: &Input, mut each: impl FnMut(&Source) -> Result<()>) -> ExitCode {
    let sources = match sources(input) {
        Ok(sources) => sources,
        Err(error) => {
            eprintln!("Error: {error:#}");
            return ExitCode::FAILURE;
        }
    };
    let mut failed = false;
    for source in &sources {
        if let Err(error) = each(source) {
            eprintln!("Error: {error:#}");
            failed = true;
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// The sheet's name in messages.
fn name(source: &Source) -> String {
    match source {
        Source::Stdin => "<stdin>".to_owned(),
        Source::File(path) => path.display().to_string(),
    }
}

fn read(source: &Source) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    source
        .open()?
        .read_to_end(&mut data)
        .with_context(|| format!("reading {}", name(source)))?;
    Ok(data)
}

/// The cue sheet `source` holds.
fn parse(source: &Source) -> Result<CueSheet> {
    CueSheet::new(&read(source)?).with_context(|| name(source))
}

/// The absolute path of each file the sheet lists, in the sheet's directory, or the current
/// directory for stdin. Symlinks are not resolved.
fn listed_files(source: &Source, sheet: &CueSheet) -> Result<Vec<PathBuf>> {
    let dir = match source {
        Source::Stdin => env::current_dir().context("reading the current directory")?,
        Source::File(path) => {
            let absolute = std::path::absolute(path)
                .with_context(|| format!("resolving {}", path.display()))?;
            absolute
                .parent()
                .with_context(|| format!("{} has no parent", absolute.display()))?
                .to_owned()
        }
    };
    Ok(sheet
        .files()
        .iter()
        .map(|file| dir.join(file.filename()))
        .collect())
}
