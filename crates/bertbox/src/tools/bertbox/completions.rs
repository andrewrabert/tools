use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args as ClapArgs, ValueEnum};
use clap_complete::Generator;
use clap_complete::shells::Fish;

use crate::tools::Tool;
use crate::{APP_NAME, Cli};

#[derive(Clone, ValueEnum)]
enum Shell {
    Fish,
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
            Shell::Fish => fish(self.all),
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

fn fish(all: bool) -> Result<()> {
    // Every name the binary answers to is a subcommand of the multicall root,
    // so building it has clap name and propagate into each of them.
    let mut cli = Cli::command_with_flags();
    cli.build();

    let mut stdout = io::stdout().lock();
    for command in cli.get_subcommands() {
        let name = command.get_name();
        if name != APP_NAME && !all {
            continue;
        }
        Fish.try_generate(command, &mut stdout)?;
        for alias in command.get_all_aliases() {
            writeln!(stdout, "complete -c {alias} -w {name}")?;
        }
    }
    stdout.flush()?;
    Ok(())
}
