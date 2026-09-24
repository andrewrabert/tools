use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use regex::Regex;
use url::Url;

use crate::tools::Tool;

const PATTERN: &str = r#"https?://[^ \\'"]*"#;

#[derive(ClapArgs)]
#[command(about = "Print the unique URLs found in a file or stdin")]
pub struct Urlextract {
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,
    #[arg(short = 'H', long, help = "print only the host of each URL")]
    host: bool,
}

impl Tool for Urlextract {
    fn run(self) -> ExitCode {
        match run(self.file, self.host) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(file: Option<PathBuf>, host_only: bool) -> Result<()> {
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
        let item = if host_only {
            match Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
            {
                Some(host) => host,
                None => continue,
            }
        } else {
            url.to_owned()
        };
        if seen.insert(item.clone()) {
            writeln!(stdout, "{item}")?;
        }
    }
    stdout.flush()?;
    Ok(())
}
