use std::io::{self, Read, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::clipboard;
use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "Copy stdin to the clipboard")]
pub struct Cbcopy {}

impl Tool for Cbcopy {
    fn run(self) -> ExitCode {
        exit_code(copy())
    }
}

#[derive(ClapArgs)]
#[command(about = "Print the clipboard")]
pub struct Cbpaste {}

impl Tool for Cbpaste {
    fn run(self) -> ExitCode {
        exit_code(paste())
    }
}

fn exit_code(result: Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        }
    }
}

fn copy() -> Result<()> {
    let mut data = Vec::new();
    io::stdin()
        .lock()
        .read_to_end(&mut data)
        .context("reading stdin")?;
    clipboard::copy(&data)
}

fn paste() -> Result<()> {
    let data = clipboard::paste()?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(&data).context("writing stdout")?;
    stdout.flush().context("writing stdout")
}
