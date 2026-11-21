use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::process::ExitCode;

use crate::tools::cue_tool::cli::Files;

/// Prints the absolute path of each file each cue sheet lists, each followed by a newline, or by
/// NUL with `-0`.
pub fn run(args: &Files) -> ExitCode {
    let terminator = if args.null { b'\0' } else { b'\n' };
    super::for_each(&args.input, |source| {
        let sheet = super::parse(source)?;
        let mut stdout = io::stdout().lock();
        for file in super::listed_files(source, &sheet)? {
            stdout.write_all(file.as_os_str().as_bytes())?;
            stdout.write_all(&[terminator])?;
        }
        Ok(())
    })
}
