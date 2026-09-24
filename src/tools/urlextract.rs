use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use regex::Regex;

use crate::tools::Tool;

const PATTERN: &str = r#"https?://[^ \\'"]*"#;

#[derive(ClapArgs)]
#[command(about = "Print the unique URLs found in a file or stdin")]
pub struct Urlextract {
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,
}

impl Tool for Urlextract {
    fn run(self) -> ExitCode {
        match run(self.file) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(file: Option<PathBuf>) -> Result<()> {
    let data = match file {
        Some(path) => fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
        None => {
            let mut data = Vec::new();
            io::stdin().lock().read_to_end(&mut data)?;
            data
        }
    };
    let text = String::from_utf8_lossy(&data);
    let pattern = Regex::new(PATTERN).expect("valid URL pattern");
    let mut seen = HashSet::new();
    let mut stdout = io::stdout().lock();
    for url in pattern.find_iter(&text).map(|m| m.as_str()) {
        if seen.insert(url) {
            writeln!(stdout, "{url}")?;
        }
    }
    stdout.flush()?;
    Ok(())
}
