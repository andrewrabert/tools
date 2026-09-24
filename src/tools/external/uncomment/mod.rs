use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::process::ExitCode;

use clap::Args as ClapArgs;

use crate::tools::Tool;

/// The uncomment CLI, compiled from an untouched copy of upstream's `src/main.rs`.
#[allow(clippy::all, clippy::pedantic, dead_code)]
mod upstream {
    include!("main.rs");

    // Upstream already defines a private `run`.
    pub(super) fn upstream_main() -> anyhow::Result<()> {
        main()
    }
}

#[derive(ClapArgs)]
#[command(
    about = "Remove comments from source files (upstream uncomment)",
    disable_help_flag = true,
    disable_version_flag = true
)]
pub struct Uncomment {
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    args: Vec<OsString>,
}

impl Tool for Uncomment {
    fn run(self) -> ExitCode {
        if std::env::args_os().skip(1).eq(self.args.iter().cloned()) {
            // Started as `uncomment ...` or `bertbox-uncomment ...`: the process argv is
            // already what upstream's `Cli::parse()` expects.
            return match upstream::upstream_main() {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("Error: {error:?}");
                    ExitCode::FAILURE
                }
            };
        }
        // Started as `bertbox uncomment ...` or with `--env-file`: argv does not match what
        // upstream parses, so re-exec with argv0 `uncomment`. ENV_NAME stays unset so the new
        // process dispatches on argv0.
        let program = match crate::dispatch::program() {
            Ok(program) => program,
            Err(error) => {
                eprintln!("Error: {:?}", error.context("running uncomment"));
                return ExitCode::FAILURE;
            }
        };
        let error = std::process::Command::new(program)
            .arg0("uncomment")
            .args(&self.args)
            .exec();
        eprintln!(
            "Error: {:?}",
            anyhow::Error::from(error).context("running uncomment")
        );
        ExitCode::FAILURE
    }
}
