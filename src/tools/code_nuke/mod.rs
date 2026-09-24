mod cli;
mod rule;

use std::path::Path;
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::tools::Tool;
pub use crate::tools::code_nuke::cli::CodeNuke;
use crate::tools::code_nuke::cli::{Passthrough, Target};

impl Tool for CodeNuke {
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

fn run(args: CodeNuke) -> Result<()> {
    let (mut command, apply) = match args.target {
        Target::Stmt {
            directory,
            pattern,
            kinds,
            passthrough,
        } => ast_grep(&rule::drop_stmt(&pattern, &kinds), &directory, passthrough),
        Target::Tests { path, passthrough } => ast_grep(&rule::drop_tests(), &path, passthrough),
        Target::Comments { path, passthrough } => uncomment(&path, passthrough),
    };
    let status = command
        .status()
        .with_context(|| format!("running {}", command.get_program().display()))?;
    if !apply {
        eprintln!("warning: dry run, no files were changed. pass -a to apply");
    }
    if !status.success() {
        bail!("{} failed: {status}", command.get_program().display());
    }
    Ok(())
}

fn ast_grep(rule: &Value, path: &Path, passthrough: Passthrough) -> (Command, bool) {
    let mut command = Command::new("ast-grep");
    command
        .args(["scan", "--inline-rules"])
        .arg(rule.to_string());
    if passthrough.apply {
        command.arg("-U");
    }
    command.args(passthrough.rest).arg(path);
    (command, passthrough.apply)
}

fn uncomment(path: &Path, passthrough: Passthrough) -> (Command, bool) {
    let mut command = Command::new("uncomment");
    command.arg(path).args([
        "-j",
        "0",
        "--remove-todo",
        "--remove-fixme",
        "--remove-doc",
        "--no-default-ignores",
    ]);
    if !passthrough.apply {
        command.args(["--dry-run", "--diff"]);
    }
    command.args(passthrough.rest);
    (command, passthrough.apply)
}
