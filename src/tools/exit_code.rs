use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, ExitCode, ExitStatus};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use crossterm::cursor;
use crossterm::style::{Attribute, Color, SetAttribute, Stylize};

use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "Run a command, then print its exit status and the time")]
pub struct ExitCodeTool {
    #[arg(
        value_name = "COMMAND",
        required = true,
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    command: Vec<OsString>,
}

impl Tool for ExitCodeTool {
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

fn run(args: ExitCodeTool) -> Result<()> {
    let (program, rest) = args.command.split_first().context("no command given")?;
    let code = match Command::new(program).args(rest).status() {
        Ok(status) => shell_code(status),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            eprintln!("{}: command not found", program.to_string_lossy());
            127
        }
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            126
        }
    };

    let color = if code == 0 {
        Color::DarkGreen
    } else {
        Color::DarkRed
    };
    let now = jiff::Timestamp::now().strftime("%Y-%m-%d %H:%M:%S");
    let mut stdout = io::stdout().lock();
    stdout.flush()?;
    // An unknown column is treated as mid-line so the status starts on its own line.
    if cursor_column() != Some(0) {
        writeln!(stdout)?;
    }
    writeln!(
        stdout,
        "{}{}",
        SetAttribute(Attribute::Reset),
        format!("Exit {code} - {now}").with(color).bold()
    )?;
    stdout.flush()?;
    Ok(())
}

/// The status as a shell reports it: 128 plus the signal for a killed command.
fn shell_code(status: ExitStatus) -> i32 {
    match (status.code(), status.signal()) {
        (Some(code), _) => code,
        (None, Some(signal)) => 128 + signal,
        (None, None) => 1,
    }
}

/// The zero-based cursor column, asked of the terminal with a cursor position report.
///
/// The report is requested over stdout, so it is only asked for when stdout is a terminal.
fn cursor_column() -> Option<u16> {
    if !io::stdout().is_terminal() {
        return None;
    }
    cursor::position().ok().map(|(column, _)| column)
}
