use std::ffi::OsString;
use std::io::{self, IsTerminal, Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode, percent_encode};

use crate::tools::Tool;

// Everything outside the RFC 3986 unreserved characters.
const RESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

#[derive(ClapArgs)]
struct Args {
    #[arg(value_name = "TEXT")]
    text: Option<OsString>,
}

#[derive(ClapArgs)]
#[command(about = "Percent-decode an argument or stdin")]
pub struct Urldecode {
    #[command(flatten)]
    args: Args,
}

#[derive(ClapArgs)]
#[command(about = "Percent-encode an argument or stdin")]
pub struct Urlencode {
    #[command(flatten)]
    args: Args,
}

impl Tool for Urldecode {
    fn run(self) -> ExitCode {
        exit_code(run(decode, self.args))
    }
}

impl Tool for Urlencode {
    fn run(self) -> ExitCode {
        exit_code(run(encode, self.args))
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

fn decode(data: &[u8]) -> Vec<u8> {
    percent_decode(data).collect()
}

fn encode(data: &[u8]) -> Vec<u8> {
    percent_encode(data, RESERVED).to_string().into_bytes()
}

fn run(filter: fn(&[u8]) -> Vec<u8>, args: Args) -> Result<()> {
    let mut stdin = io::stdin().lock();
    let data = if stdin.is_terminal() {
        args.text
            .context("expected TEXT or piped stdin")?
            .into_vec()
    } else {
        let mut data = Vec::new();
        stdin.read_to_end(&mut data)?;
        data
    };

    let mut stdout = io::stdout().lock();
    stdout.write_all(&filter(&data))?;
    stdout.flush()?;
    Ok(())
}
