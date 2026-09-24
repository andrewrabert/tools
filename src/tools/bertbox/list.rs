use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args as ClapArgs, Command, Subcommand};
use serde::Serialize;

use crate::tools::Tool;
use crate::{APP_NAME, AnyTool};

#[derive(ClapArgs)]
#[command(about = "List every tool")]
pub struct List {
    /// Print groups with their descriptions and tools as JSON
    #[arg(long)]
    json: bool,
}

// Fields are declared in the order they are printed.
#[derive(Serialize)]
struct GroupEntry {
    description: Option<&'static str>,
    group: &'static str,
    tools: Vec<ToolEntry>,
}

#[derive(Serialize)]
struct ToolEntry {
    description: Option<String>,
    name: &'static str,
}

impl Tool for List {
    fn run(self) -> ExitCode {
        let result = if self.json { json() } else { names() };
        match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn names() -> Result<()> {
    let mut stdout = io::stdout().lock();
    for name in AnyTool::groups().flat_map(|group| group.names) {
        writeln!(stdout, "{name}")?;
    }
    stdout.flush()?;
    Ok(())
}

fn json() -> Result<()> {
    let commands = AnyTool::augment_subcommands(Command::new(APP_NAME));

    let groups: Vec<GroupEntry> = AnyTool::groups()
        .map(|group| GroupEntry {
            description: group.description,
            group: group.heading,
            tools: group
                .names
                .iter()
                .map(|name| ToolEntry {
                    description: commands
                        .find_subcommand(name)
                        .and_then(Command::get_about)
                        .map(ToString::to_string),
                    name,
                })
                .collect(),
        })
        .collect();

    let rendered = serde_json::to_string_pretty(&groups).context("rendering the tool list")?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{rendered}")?;
    stdout.flush()?;
    Ok(())
}
