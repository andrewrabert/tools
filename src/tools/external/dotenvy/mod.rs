use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args as ClapArgs;

use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "Run a command using the environment in a .env file (upstream dotenvy)")]
pub struct Dotenvy {
    /// Use a specific .env file (defaults to .env)
    #[arg(short, long, value_name = "FILE")]
    file: Option<PathBuf>,

    #[arg(
        value_name = "COMMAND",
        required = true,
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    command: Vec<OsString>,
}

impl Tool for Dotenvy {
    fn run(self) -> ExitCode {
        let loaded = match &self.file {
            Some(file) => ::dotenvy::from_filename(file),
            None => ::dotenvy::dotenv(),
        };
        if let Err(error) = loaded {
            eprintln!(
                "Error: {:?}",
                anyhow::Error::from(error).context("loading environment")
            );
            return ExitCode::FAILURE;
        }
        let mut command = self.command.into_iter();
        let Some(program) = command.next() else {
            eprintln!("Error: missing COMMAND");
            return ExitCode::FAILURE;
        };
        let error = std::process::Command::new(&program).args(command).exec();
        eprintln!(
            "Error: {:?}",
            anyhow::Error::from(error).context(format!("running {}", program.display()))
        );
        ExitCode::FAILURE
    }
}
