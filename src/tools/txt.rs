use std::ffi::OsString;
use std::io::{self, BufRead, Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::tools::Tool;
use crate::{clipboard, input};

const CHUNK_SIZE: usize = 64 * 1024;

#[derive(ClapArgs)]
struct Args {
    #[arg(value_name = "TEXT")]
    text: Option<OsString>,
}

#[derive(ClapArgs)]
struct ClipboardArgs {
    #[arg(value_name = "TEXT")]
    text: Vec<String>,
}

#[derive(ClapArgs)]
#[command(about = "aLtErNaTiNg cAsE arguments, stdin, or the clipboard, copied to the clipboard")]
pub struct Alternating {
    #[command(flatten)]
    args: ClipboardArgs,
}

#[derive(ClapArgs)]
#[command(about = "Lowercase an argument or stdin")]
pub struct Lower {
    #[command(flatten)]
    args: Args,
}

#[derive(ClapArgs)]
#[command(about = "Strip surrounding whitespace from stdin")]
pub struct Trim {}

#[derive(ClapArgs)]
#[command(about = "Strip each line of stdin")]
pub struct Trimlines {}

#[derive(ClapArgs)]
#[command(about = "Uppercase an argument or stdin")]
pub struct Upper {
    #[command(flatten)]
    args: Args,
}

#[derive(ClapArgs)]
#[command(about = "Title Case arguments, stdin, or the clipboard, copied to the clipboard")]
pub struct UpperFirst {
    #[command(flatten)]
    args: ClipboardArgs,
}

impl Tool for Alternating {
    fn run(self) -> ExitCode {
        exit_code(clipboard_filter(alternating, self.args))
    }
}

impl Tool for Lower {
    fn run(self) -> ExitCode {
        exit_code(run(Case::Lower, self.args))
    }
}

impl Tool for Trim {
    fn run(self) -> ExitCode {
        exit_code(trim())
    }
}

impl Tool for Trimlines {
    fn run(self) -> ExitCode {
        exit_code(trimlines())
    }
}

impl Tool for Upper {
    fn run(self) -> ExitCode {
        exit_code(run(Case::Upper, self.args))
    }
}

impl Tool for UpperFirst {
    fn run(self) -> ExitCode {
        exit_code(clipboard_filter(upper_first, self.args))
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

enum Case {
    Lower,
    Upper,
}

impl Case {
    fn apply(&self, bytes: &mut [u8]) {
        match self {
            Case::Lower => bytes.make_ascii_lowercase(),
            Case::Upper => bytes.make_ascii_uppercase(),
        }
    }
}

fn run(case: Case, args: Args) -> Result<()> {
    let mut stdout = io::stdout().lock();
    match args.text {
        Some(text) => {
            let mut bytes = text.into_vec();
            case.apply(&mut bytes);
            stdout.write_all(&bytes)?;
        }
        None => {
            let mut stdin = io::stdin().lock();
            let mut chunk = vec![0; CHUNK_SIZE];
            loop {
                let read = stdin.read(&mut chunk)?;
                if read == 0 {
                    break;
                }
                case.apply(&mut chunk[..read]);
                stdout.write_all(&chunk[..read])?;
            }
        }
    }
    stdout.flush()?;
    Ok(())
}

fn alternating(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut upper = false;
    for c in text.chars() {
        if upper {
            result.extend(c.to_uppercase());
        } else {
            result.extend(c.to_lowercase());
        }
        upper = !upper;
    }
    result
}

fn upper_first(text: &str) -> String {
    let mut words = Vec::new();
    for word in text.split_whitespace() {
        let mut chars = word.chars();
        let mut titled = String::with_capacity(word.len());
        if let Some(first) = chars.next() {
            titled.extend(first.to_uppercase());
        }
        titled.push_str(&chars.as_str().to_lowercase());
        words.push(titled);
    }
    words.join(" ")
}

fn clipboard_filter(filter: fn(&str) -> String, args: ClipboardArgs) -> Result<()> {
    let data = input::read(args.text)?;
    let text = String::from_utf8(data).context("input is not UTF-8")?;
    let result = filter(&text);
    clipboard::copy(result.as_bytes())?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(result.as_bytes())?;
    stdout.flush()?;
    Ok(())
}

fn trim() -> Result<()> {
    let mut text = String::new();
    io::stdin().lock().read_to_string(&mut text)?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(text.trim().as_bytes())?;
    stdout.flush()?;
    Ok(())
}

fn trimlines() -> Result<()> {
    let mut stdout = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        writeln!(stdout, "{}", line?.trim())?;
    }
    stdout.flush()?;
    Ok(())
}
