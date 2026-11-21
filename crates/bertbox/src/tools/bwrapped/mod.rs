//! Run a program inside a sandbox with filesystem isolation: bubblewrap on Linux, seatbelt
//! (sandbox-exec) on macOS.
mod bubblewrap;
mod noted;
mod paths;
mod sandbox;
mod seatbelt;
mod settings;

use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use signal_hook::consts::{SIGINT, SIGQUIT};

use self::bubblewrap::Bubblewrap;
use self::noted::NotesServer;
use self::sandbox::{Host, Invocation, Noted, Sandbox};
use self::seatbelt::Seatbelt;
use self::settings::{SandboxArgs, Settings};
use crate::tools::Tool;
use crate::which;

#[derive(ClapArgs)]
#[command(about = "Run a program inside a sandbox with filesystem isolation")]
pub struct Bwrapped {
    #[command(flatten)]
    sandbox: SandboxArgs,
    /// Disable noted integration and remove NOTED_ variables
    #[arg(long)]
    no_noted: bool,
    /// Write the effective settings as TOML; a directory means its .bwrapped.conf
    #[arg(short, long, value_name = "PATH")]
    write_config: Option<PathBuf>,
    /// Program to run inside the sandbox
    #[arg(value_name = "PROGRAM")]
    program: String,
    /// Arguments passed to PROGRAM
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    program_args: Vec<OsString>,
}

impl Tool for Bwrapped {
    fn run(self) -> ExitCode {
        let result = if cfg!(target_os = "macos") {
            run::<Seatbelt>(self)
        } else {
            run::<Bubblewrap>(self)
        };
        result.unwrap_or_else(|error| {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        })
    }
}

fn run<S: Sandbox>(args: Bwrapped) -> Result<ExitCode> {
    let host = Host::current()?;
    let config = settings::find_config(&host.work_dir);
    which::first(S::BIN).with_context(|| format!("{} not found", S::BIN))?;
    let settings = Settings::resolve(config.as_deref(), args.sandbox, &host.work_dir)?;
    if let Some(path) = &args.write_config {
        settings.write(path)?;
    }
    let program = resolve_program(&args.program)?;
    let server = if args.no_noted {
        None
    } else {
        Some(NotesServer::start(
            settings.noted_scope.as_deref().unwrap_or_default(),
        )?)
    };
    let noted = match &server {
        None => Noted::Disabled,
        Some(server) => Noted::Enabled {
            endpoint: server.endpoint(),
            source: settings.noted_source.as_deref().unwrap_or(&args.program),
        },
    };
    let mut sandbox = S::new(&host)?;
    sandbox::build(
        &mut sandbox,
        &host,
        &Invocation {
            settings: &settings,
            noted,
            program: &program,
            program_args: &args.program_args,
            config: config.as_deref(),
        },
    )?;
    run_sandbox(sandbox.command())
}

/// The file PATH resolves `name` to, or `name` itself when it names a path.
fn resolve_program(name: &str) -> Result<PathBuf> {
    if name.contains('/') {
        let path = Path::new(name);
        anyhow::ensure!(path.is_file(), "{name} not found");
        return Ok(path.to_owned());
    }
    which::first(name).with_context(|| format!("{name} not found on PATH"))
}

/// Runs the sandbox to completion and returns its exit status.
///
/// SIGINT and SIGQUIT are handled rather than ignored: exec resets a handled signal to its
/// default, so Ctrl-C still reaches the sandboxed program while the wrapper keeps waiting.
fn run_sandbox(mut command: Command) -> Result<ExitCode> {
    let received = Arc::new(AtomicBool::new(false));
    for signal in [SIGINT, SIGQUIT] {
        signal_hook::flag::register(signal, Arc::clone(&received))
            .context("installing signal handlers")?;
    }
    let status = command
        .status()
        .with_context(|| format!("cannot launch {}", command.get_program().display()))?;
    Ok(ExitCode::from(shell_code(status)))
}

/// The status as a shell reports it: 128 plus the signal for a killed command.
fn shell_code(status: ExitStatus) -> u8 {
    let code = match (status.code(), status.signal()) {
        (Some(code), _) => code,
        (None, Some(signal)) => 128 + signal,
        (None, None) => 1,
    };
    u8::try_from(code).unwrap_or(1)
}
