use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;

use crate::atomic_write;
use crate::source::Source;
use crate::tools::cue_tool::cli::Fmt;

const TEMP_PREFIX: &str = ".cue_tool_fmt_";

/// Prints each cue sheet in a uniform layout, or with `--in-place` rewrites it.
pub fn run(args: &Fmt) -> ExitCode {
    super::for_each(&args.input, |source| {
        let sheet = super::parse(source)?;
        let text = sheet.to_string();
        match (source, args.in_place) {
            (Source::File(path), true) => rewrite(path, &text),
            (Source::File(_), false) | (Source::Stdin, _) => {
                io::stdout().lock().write_all(text.as_bytes())?;
                Ok(())
            }
        }
    })
}

/// Replaces the file at `path` with `text`, unless it already holds `text`.
fn rewrite(path: &Path, text: &str) -> Result<()> {
    if fs::read(path).is_ok_and(|data| data == text.as_bytes()) {
        return Ok(());
    }
    let temp = atomic_write::sibling(TEMP_PREFIX, path)?;
    fs::write(&temp, text)?;
    atomic_write::replace(temp, path)
}
