use std::env;
use std::ffi::OsString;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{self, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use anyhow::{Context, Result, anyhow};
use signal_hook::consts::SIGINT;

use crate::dispatch;
use crate::tools::fanpipe::{self, Topic, Until};

const MARKER: &str = "LMK";
const TOOL: &str = "lmk";

pub struct Pane(OsString);

pub enum Session {
    Outside,
    Inside(Pane),
}

impl Session {
    pub fn detect() -> Self {
        let marked = env::var_os(MARKER).is_some_and(|value| !value.is_empty());
        match env::var_os("TMUX_PANE") {
            Some(pane) if marked => Session::Inside(Pane(pane)),
            _ => Session::Outside,
        }
    }
}

pub fn host(command: &[OsString]) -> Result<()> {
    let socket = env::temp_dir().join(format!("lmk-{}", process::id()));
    let error = Command::new("tmux")
        .env_remove("TMUX")
        .env(MARKER, "1")
        .arg("-S")
        .arg(socket)
        .args(["-f", "/dev/null"])
        .args(["unbind", "-a", ";"])
        .args([
            "set-option",
            "-g",
            "terminal-overrides",
            "*:smcup@:rmcup@",
            ";",
        ])
        .args(["set-option", "-g", "status", "off", ";"])
        .args(["set-option", "-g", "destroy-unattached", "on", ";"])
        .args(["set-option", "-g", "detach-on-destroy", "on", ";"])
        .args(["new-session", "-e", &environment()])
        .arg(dispatch::program()?)
        .arg(TOOL)
        .args(command)
        .args([";", "attach"])
        .exec();
    Err(error).context("running tmux")
}

pub fn watch(topic: &Topic, pane: &Pane, command: &[OsString]) -> Result<()> {
    let (program, arguments) = command.split_first().context("missing command")?;
    let interruptible = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register_conditional_default(SIGINT, Arc::clone(&interruptible))
        .context("handling SIGINT")?;

    thread::scope(|scope| {
        let rerun = scope.spawn(|| -> Result<()> {
            fanpipe::subscribe(topic, Until::FirstMessage, &mut io::sink())?;
            respawn(pane, command)
        });

        if let Err(e) = Command::new(program).args(arguments).status() {
            eprintln!("{}: {e}", program.display());
        }
        interruptible.store(true, Ordering::Relaxed);

        rerun
            .join()
            .map_err(|_| anyhow!("notification thread panicked"))?
    })
}

fn environment() -> String {
    format!("{}={}", dispatch::ENV_NAME, dispatch::ENV_VALUE)
}

fn respawn(pane: &Pane, command: &[OsString]) -> Result<()> {
    Command::new("tmux")
        .args(["respawn-pane", "-k", "-e", &environment(), "-t"])
        .arg(&pane.0)
        .arg(dispatch::program()?)
        .arg(TOOL)
        .args(command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("running tmux")?;
    Ok(())
}
