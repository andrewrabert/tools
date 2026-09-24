use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use crate::tools::cue_tool::cli::Json;

/// Prints the tokens of each cue sheet as JSON: pretty-printed for one sheet on a terminal, and
/// otherwise one line per sheet.
pub fn run(args: &Json) -> ExitCode {
    let pretty = !args.input.recursive && io::stdout().is_terminal();
    super::for_each(&args.input, |source| {
        let sheet = super::parse(source)?;
        let mut stdout = io::stdout().lock();
        if pretty {
            serde_json::to_writer_pretty(&mut stdout, sheet.tokens())?;
        } else {
            serde_json::to_writer(&mut stdout, sheet.tokens())?;
        }
        writeln!(stdout)?;
        Ok(())
    })
}
