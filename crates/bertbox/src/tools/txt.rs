use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args as ClapArgs, ValueEnum};
use tempfile::NamedTempFile;

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
#[command(about = "Rewrite the line endings of files in place")]
pub struct LineEnd {
    #[arg(short, long, value_enum)]
    mode: LineEnding,
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum LineEnding {
    Dos,
    Unix,
}

impl LineEnding {
    fn bytes(self) -> &'static [u8] {
        match self {
            LineEnding::Dos => b"\r\n",
            LineEnding::Unix => b"\n",
        }
    }
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

impl Tool for LineEnd {
    fn run(self) -> ExitCode {
        exit_code(line_end(self))
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

fn line_end(args: LineEnd) -> Result<()> {
    for path in &args.paths {
        println!("{}", path.display());
        rewrite_line_endings(path, args.mode)?;
    }
    Ok(())
}

fn rewrite_line_endings(path: &Path, mode: LineEnding) -> Result<()> {
    // Resolved so a symlink keeps pointing at the rewritten file.
    let path = fs::canonicalize(path).with_context(|| format!("resolving {}", path.display()))?;
    let data = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let converted = convert_line_endings(&data, mode.bytes());
    if converted == data {
        return Ok(());
    }
    let parent = path.parent().context("file has no parent directory")?;
    let permissions = fs::metadata(&path)
        .with_context(|| format!("reading {}", path.display()))?
        .permissions();
    let mut temp = NamedTempFile::new_in(parent)
        .with_context(|| format!("creating a temporary file in {}", parent.display()))?;
    temp.write_all(&converted)?;
    temp.as_file().set_permissions(permissions)?;
    temp.persist(&path)
        .with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// Ends every line with `ending`, including an unterminated last line.
fn convert_line_endings(data: &[u8], ending: &[u8]) -> Vec<u8> {
    let mut converted = Vec::with_capacity(data.len() + data.len() / 32);
    if data.is_empty() {
        return converted;
    }
    let body = data.strip_suffix(b"\n").unwrap_or(data);
    for line in body.split(|byte| *byte == b'\n') {
        converted.extend_from_slice(line.strip_suffix(b"\r").unwrap_or(line));
        converted.extend_from_slice(ending);
    }
    converted
}
