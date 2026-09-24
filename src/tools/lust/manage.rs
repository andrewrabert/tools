use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args as ClapArgs, Subcommand};

use crate::tools::Tool;
use crate::tools::lust::{LustDir, Project, ProjectDir};

const AGENTS: &str = "\
# `lust` — personal per-project justfile

`lust` runs `just` against a personal justfile stored *outside* the current
project. Use it for personal recipes that should not be committed.

- Each project gets a directory under the user's config directory, named by
  the sha256 of its key. The key is the git remote URL (`upstream`, then
  `origin`), else the absolute path of the git root, else the absolute working
  directory. `lust-manage dir` prints it.
- `lust` (no args) — list recipes.
- `lust <recipe>` — run a recipe; every argument goes to just.
- `lust-manage dir` — print the project's lust directory.
- `lust-manage file` — print the path to the personal justfile.
- `lust-manage repos` — list every lust directory and its key.
";

#[derive(ClapArgs)]
#[command(about = "Inspect the personal justfiles lust runs")]
pub struct LustManage {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "print agent guidance for using lust")]
    Agents,
    #[command(about = "print the current project's lust directory")]
    Dir,
    #[command(about = "print the current project's justfile path")]
    File,
    #[command(about = "list every lust directory and its key")]
    Repos,
}

impl Tool for LustManage {
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

fn run(args: LustManage) -> Result<()> {
    let mut stdout = io::stdout().lock();
    match args.command {
        Command::Agents => stdout.write_all(AGENTS.as_bytes())?,
        Command::Dir => {
            let project_dir = current_project_dir()?;
            writeln!(stdout, "{}", project_dir.path.display())?;
        }
        Command::File => {
            let project_dir = current_project_dir()?;
            writeln!(stdout, "{}", project_dir.justfile.display())?;
        }
        Command::Repos => {
            for (path, source) in LustDir::locate()?.projects()? {
                writeln!(stdout, "{}\t{source}", path.display())?;
            }
        }
    }
    stdout.flush()?;
    Ok(())
}

fn current_project_dir() -> Result<ProjectDir> {
    let lust_dir = LustDir::locate()?;
    let project = Project::resolve()?;
    ProjectDir::ensure(&lust_dir, &project)
}
