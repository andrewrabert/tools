mod cli;
mod host;
mod pikvm;
mod roots;
mod script;

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use signal_hook::consts::{SIGINT, SIGTERM};

use crate::tools::Tool;
pub use crate::tools::full_update::cli::FullUpdate;
use crate::tools::full_update::host::Host;
use crate::tools::full_update::pikvm::RootFilesystem;
use crate::tools::full_update::roots::Roots;
use crate::tools::full_update::script::ScriptName;

const YELLOW: &str = "\x1b[1;33m";
const RESET_COLOR: &str = "\x1b[0m";
const BREW_GNUBIN: &str = "/opt/homebrew/opt/coreutils/libexec/gnubin";
const PIKVM_SCRIPTS: [&str; 4] = ["00-dotfiles", "10-pikvm", "60-tmux", "60-zsh"];

struct Runner {
    host: OsString,
    path: Option<OsString>,
    interrupted: Arc<AtomicBool>,
}

impl Runner {
    fn new(host: &Host) -> Result<Self> {
        let interrupted = Arc::new(AtomicBool::new(false));
        for signal in [SIGINT, SIGTERM] {
            signal_hook::flag::register(signal, Arc::clone(&interrupted))
                .context("handling signals")?;
        }
        let path = if Path::new(BREW_GNUBIN).is_dir() {
            let inherited = env::var_os("PATH").unwrap_or_default();
            let dirs = std::iter::once(BREW_GNUBIN.into()).chain(env::split_paths(&inherited));
            Some(env::join_paths(dirs).context("extending PATH")?)
        } else {
            None
        };
        Ok(Runner {
            host: host.name(),
            path,
            interrupted,
        })
    }

    fn run(&self, script: &Path, args: &[OsString]) -> Result<()> {
        if let Some(name) = script.file_name() {
            announce(&format!("Running {} ...", name.display()));
        }
        let mut command = Command::new(script);
        command.args(args).env(host::VARIABLE, &self.host);
        if let Some(path) = &self.path {
            command.env("PATH", path);
        }
        let status = command
            .status()
            .with_context(|| format!("running {}", script.display()))?;
        if !status.success() {
            bail!("{} failed: {status}", script.display());
        }
        if self.interrupted.load(Ordering::Relaxed) {
            bail!("interrupted");
        }
        Ok(())
    }

    fn run_named(&self, roots: &Roots, name: &ScriptName, args: &[OsString]) -> Result<()> {
        for dir in roots.script_dirs() {
            let script = dir.join(name.as_os_str());
            if script.is_file() {
                self.run(&script, args)?;
            }
        }
        Ok(())
    }

    fn run_all(&self, dir: &Path, skip: &[ScriptName]) -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }
        let dotfiles = ScriptName::from(ScriptName::DOTFILES);
        let refresh = dir.join(dotfiles.as_os_str());
        if refresh.is_file() {
            if skip.contains(&dotfiles) {
                announce(&format!("Skipping {dotfiles}"));
            } else {
                self.run(&refresh, &["--system".into()])?;
            }
        }
        for name in script_names(dir)? {
            if name == dotfiles {
                continue;
            }
            if skip.contains(&name) {
                announce(&format!("Skipping {name}"));
                continue;
            }
            let script = dir.join(name.as_os_str());
            if script.is_file() {
                self.run(&script, &[])?;
            }
        }
        Ok(())
    }
}

impl Tool for FullUpdate {
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

fn run(args: FullUpdate) -> Result<()> {
    let host = Host::resolve()?;
    let runner = Runner::new(&host)?;
    let roots = Roots::from_env()?;
    let _root_filesystem = match host {
        Host::Pikvm => Some(RootFilesystem::make_writable()?),
        Host::Other(_) => None,
    };

    match (args.script, &host) {
        (Some(name), _) => runner.run_named(&roots, &name, &args.script_args),
        (None, Host::Pikvm) => {
            for name in PIKVM_SCRIPTS {
                runner.run_named(&roots, &ScriptName::from(name), &[])?;
            }
            Ok(())
        }
        (None, Host::Other(_)) => {
            for dir in roots.script_dirs() {
                runner.run_all(&dir, &args.skip)?;
            }
            Ok(())
        }
    }
}

fn script_names(dir: &Path) -> Result<Vec<ScriptName>> {
    let entries = fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let name = entry.file_name();
        if !name.as_encoded_bytes().starts_with(b".") {
            names.push(ScriptName::from(name));
        }
    }
    names.sort();
    Ok(names)
}

fn announce(message: &str) {
    println!("{YELLOW}{message}{RESET_COLOR}");
}
