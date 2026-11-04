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

#[derive(Serialize)]
struct Listing {
    groups: Vec<GroupEntry>,
}

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
    let rendered = listing()?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{rendered}")?;
    stdout.flush()?;
    Ok(())
}

/// The groups, their descriptions, and their tools, rendered as pretty-printed JSON.
fn listing() -> Result<String> {
    let commands = AnyTool::augment_subcommands(Command::new(APP_NAME));

    let groups = AnyTool::groups()
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

    serde_json::to_string_pretty(&Listing { groups }).context("rendering the tool list")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use snapbox::{Data, assert_data_eq};

    const README_TEMPLATE: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md.j2"));

    /// A file at the repository root, as the expected value of an assert.
    fn root_file(name: &str) -> Data {
        Data::read_from(&Path::new(env!("CARGO_MANIFEST_DIR")).join(name), None).raw()
    }

    #[test]
    fn committed_readme_matches_tools() {
        let listing: serde_json::Value = serde_json::from_str(&super::listing().unwrap()).unwrap();
        let mut env = minijinja::Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        env.set_syntax(
            minijinja::syntax::SyntaxConfig::builder()
                .keep_trailing_newline(true)
                .build()
                .unwrap(),
        );
        let context = minijinja::Value::from(minijinja::value::Serde(&listing));
        let rendered = env.render_str(README_TEMPLATE, context).unwrap();
        assert_data_eq!(rendered, root_file("README.md"));
    }
}
