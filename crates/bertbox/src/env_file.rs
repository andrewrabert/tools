use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Arg, ArgAction, Command, ValueHint};

const ID: &str = "env_file";

pub fn with_flag(command: Command) -> Command {
    command.arg(
        Arg::new(ID)
            .long("env-file")
            .value_name("PATH")
            .value_parser(clap::value_parser!(PathBuf))
            .value_hint(ValueHint::FilePath)
            .action(ArgAction::Append)
            .global(true)
            .help("Dotenv file that updates the environment; repeatable, applied in order"),
    )
}

pub fn load(command: &Command) -> Result<()> {
    for path in env_files(command.clone())? {
        dotenvy::from_path_override(&path)
            .with_context(|| format!("loading {}", path.display()))?;
    }
    Ok(())
}

fn env_files(command: Command) -> Result<Vec<PathBuf>> {
    // The flag is global, so a subcommand has it whenever its parent does;
    // a command given the flag only on its subcommands lacks it at the top.
    let mut has_flag = command.get_arguments().any(|arg| arg.get_id() == ID);
    let Ok(mut matches) = command.ignore_errors(true).try_get_matches() else {
        return Ok(Vec::new());
    };
    while let Some((_, sub_matches)) = matches.remove_subcommand() {
        matches = sub_matches;
        has_flag = true;
    }
    if !has_flag {
        return Ok(Vec::new());
    }
    let paths = matches
        .try_remove_many::<PathBuf>(ID)
        .context("reading --env-file")?;
    Ok(paths.into_iter().flatten().collect())
}
