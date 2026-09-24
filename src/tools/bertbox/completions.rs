use std::io::{self, Write};
use std::iter;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args as ClapArgs, ValueEnum};
use clap_complete::Generator;
use clap_complete::shells::Zsh;

use crate::tools::Tool;
use crate::{APP_NAME, Cli};

#[derive(Clone, ValueEnum)]
enum Shell {
    Zsh,
}

#[derive(ClapArgs)]
#[command(about = "Generate shell completions")]
pub struct Completions {
    #[arg(value_name = "SHELL")]
    shell: Shell,

    /// Also complete every standalone tool name
    #[arg(short, long)]
    all: bool,
}

impl Tool for Completions {
    fn run(self) -> ExitCode {
        let result = match self.shell {
            Shell::Zsh => zsh(self.all),
        };
        match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn zsh(all: bool) -> Result<()> {
    // Every name the binary answers to is a subcommand of the multicall root,
    // so building it has clap name and propagate into each of them.
    let mut cli = Cli::command_with_flags();
    cli.build();

    let mut functions = String::new();
    let mut services = Vec::new();
    for command in cli.get_subcommands() {
        let name = command.get_name();
        let standalone = name != APP_NAME;
        if standalone && !all {
            continue;
        }
        let mut script = Vec::new();
        Zsh.try_generate(command, &mut script)?;
        functions.push_str(strip_loader(&String::from_utf8(script)?, name)?);
        let names: Vec<&str> = iter::once(name).chain(command.get_all_aliases()).collect();
        services.push((name, names.join(" ")));
    }

    let mut stdout = io::stdout().lock();
    let all_names: Vec<&str> = services.iter().map(|(_, names)| names.as_str()).collect();
    writeln!(stdout, "#compdef {}", all_names.join(" "))?;
    stdout.write_all(functions.as_bytes())?;
    for (name, names) in &services {
        writeln!(stdout, "compdef _{name} {names}")?;
    }
    writeln!(stdout, "if [ \"$funcstack[1]\" = \"_{APP_NAME}\" ]; then")?;
    writeln!(stdout, "    case $service in")?;
    for (name, names) in &services {
        writeln!(
            stdout,
            "        ({}) _{name} \"$@\" ;;",
            names.replace(' ', "|")
        )?;
    }
    writeln!(stdout, "    esac")?;
    writeln!(stdout, "fi")?;
    stdout.flush()?;
    Ok(())
}

fn strip_loader<'a>(script: &'a str, name: &str) -> Result<&'a str> {
    let header = format!("#compdef {name}\n");
    let loader = format!(
        "if [ \"$funcstack[1]\" = \"_{name}\" ]; then\n    _{name} \"$@\"\nelse\n    compdef _{name} {name}\nfi\n"
    );
    script
        .strip_prefix(&header)
        .and_then(|script| script.strip_suffix(&loader))
        .with_context(|| format!("unexpected zsh completion layout for {name}"))
}
