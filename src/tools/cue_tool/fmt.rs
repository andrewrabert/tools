use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Result, bail};

use crate::atomic_write;
use crate::cue::CueSheet;
use crate::source::Source;
use crate::tools::cue_tool::cli::Fmt;

const TEMP_PREFIX: &str = ".cue_tool_fmt_";

/// Prints each cue sheet in a uniform layout, or with `--in-place` rewrites it.
pub fn run(args: &Fmt) -> ExitCode {
    super::for_each(&args.input, |source| {
        let sheet = super::parse(source)?;
        let text = sheet.to_string();
        if CueSheet::new(text.as_bytes())?.tokens() != sheet.tokens() {
            bail!(
                "{}: formatting would change its tokens",
                super::name(source)
            );
        }
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
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp = atomic_write::sibling(dir, TEMP_PREFIX, path).create()?;
    fs::write(&temp, text)?;
    atomic_write::replace(temp, path)
}
