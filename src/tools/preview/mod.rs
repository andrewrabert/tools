mod cli;
mod config;
mod kind;
mod mime;
mod render;

use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Result, anyhow, bail};

use crate::dispatch;
use crate::tools::Tool;
use crate::tools::pathcacher::cached_output;
pub use crate::tools::preview::cli::Preview;
use crate::tools::preview::config::{Caching, Color, Config, Headers};

const TOOL: &str = "preview";

impl Tool for Preview {
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

fn run(args: Preview) -> Result<()> {
    let config = Config::from(args);
    for (index, file) in config.files.iter().enumerate() {
        if index > 0 {
            println!();
        }
        if let Headers::Multiple | Headers::Full = config.headers {
            println!("#### preview: {}", file.display());
        }
        match config.caching {
            Caching::Pathcacher => cached(file, &config)?,
            Caching::Bypass => {
                let (data, result) = match render::file(file, &config.color) {
                    Ok(data) => (data, Ok(())),
                    Err(failure) => (failure.partial, Err(failure.error)),
                };
                let mut stdout = io::stdout().lock();
                stdout.write_all(&data)?;
                stdout.flush()?;
                result?;
            }
        }
    }
    Ok(())
}

fn cached(file: &Path, config: &Config) -> Result<()> {
    let mut name = String::from(TOOL);
    if let Color::Always = config.color {
        name.push_str("-color");
    }
    if let Headers::Full = config.headers {
        name.push_str("-full");
    }
    let program = dispatch::program()?;
    let data = cached_output(file, &name, || {
        render::file(file, &config.color).map_err(|failure| {
            eprintln!("Error: {:?}", failure.error);
            anyhow!("{} failed: exit status: 1", program.display())
        })
    });
    let data = match data {
        Ok(data) => data,
        Err(error) => {
            eprintln!("Error: {error:?}");
            bail!("pathcacher failed: exit status: 1");
        }
    };
    let mut stdout = io::stdout().lock();
    stdout.write_all(&data)?;
    stdout.flush()?;
    Ok(())
}
