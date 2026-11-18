mod apply;
mod cli;
mod export;
mod files;
mod paths;
mod plugins;
mod spec;
mod state;
mod update;

use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

use crate::tools::Tool;
pub use crate::tools::claude_config::cli::ClaudeConfig;
use crate::tools::claude_config::cli::Command;
use crate::tools::claude_config::files::Log;
use crate::tools::claude_config::spec::Spec;
use crate::tools::claude_config::state::ClaudeDir;

impl Tool for ClaudeConfig {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: ClaudeConfig) -> Result<ExitCode> {
    let log = Log { quiet: args.quiet };
    match args.command {
        Command::Apply { dir } => apply::run(&dir, &ClaudeDir::locate()?, log),
        Command::Export {
            dir,
            force,
            dry_run,
        } => export::run(&dir, force, dry_run, &ClaudeDir::locate()?, log),
        Command::New { kind } => {
            print!("{}", kind.text());
            Ok(ExitCode::SUCCESS)
        }
        Command::Validate { spec } => validate(spec).map(|()| ExitCode::SUCCESS),
        Command::Update => update::run(&ClaudeDir::locate()?, log),
    }
}

fn validate(spec: Option<PathBuf>) -> Result<()> {
    match spec {
        Some(path) => Spec::read(&path).map(drop),
        None if io::stdin().is_terminal() => {
            bail!("spec file required (provide path or pipe to stdin)")
        }
        None => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .context("reading stdin")?;
            Spec::parse(&text).map(drop)
        }
    }
}
