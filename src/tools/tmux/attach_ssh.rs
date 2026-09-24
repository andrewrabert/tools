use std::env;
use std::ffi::{OsStr, OsString};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};

use std::path::Path;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::tmux;
use crate::tools::Tool;
use crate::tools::tmux::exit_code;

const INHIBIT_IDLE_HOSTS: [&str; 3] = ["mars", "phobos", "lounge-htpc"];

#[derive(ClapArgs)]
#[command(
    about = "SSH to a host and attach to its tmux session, in a new tmux window when in tmux"
)]
pub struct AttachSsh {
    #[arg(short = 'c', long, help = "open in the current tmux pane")]
    pub current_pane: bool,
    #[arg(
        long,
        help = "open in the current tmux window and name it after the host"
    )]
    pub current_window: bool,
    #[arg(
        long,
        value_name = "SHELL",
        help = "remote login shell [default: the name of the local $SHELL]"
    )]
    pub shell: Option<OsString>,
    #[arg(value_name = "SSH_HOST")]
    pub host: String,
}

impl Tool for AttachSsh {
    fn run(self) -> ExitCode {
        let placement = if self.current_window {
            Placement::CurrentWindow
        } else if self.current_pane {
            Placement::CurrentPane
        } else {
            Placement::NewWindow
        };
        let result =
            Shell::resolve(self.shell).and_then(|shell| connect(&self.host, &shell, placement));
        exit_code(result)
    }
}

pub struct Shell(OsString);

impl Shell {
    pub fn resolve(flag: Option<OsString>) -> Result<Self> {
        if let Some(shell) = flag {
            return Ok(Shell(shell));
        }
        let local = env::var_os("SHELL")
            .filter(|shell| !shell.is_empty())
            .context("SHELL is not set; pass --shell")?;
        let name = Path::new(&local)
            .file_name()
            .with_context(|| format!("SHELL has no file name: {}", local.display()))?;
        Ok(Shell(name.to_owned()))
    }
}

pub enum Placement {
    NewWindow,
    CurrentPane,
    CurrentWindow,
}

pub fn connect(host: &str, shell: &Shell, placement: Placement) -> Result<()> {
    let in_tmux = tmux::current_socket().is_ok();
    let name = window_name(host);
    match placement {
        Placement::NewWindow if in_tmux => {
            let new_window = ["new-window", "-n", &name, "ssh"].map(OsStr::new);
            tmux::run(new_window.into_iter().chain(ssh_args(host, shell)))
                .map_err(anyhow::Error::msg)?;
            mark_window()
        }
        Placement::NewWindow | Placement::CurrentPane => exec_ssh(host, shell),
        Placement::CurrentWindow => {
            tmux::run(["rename-window", &name]).map_err(anyhow::Error::msg)?;
            mark_window()?;
            exec_ssh(host, shell)
        }
    }
}

fn ssh_args<'a>(host: &'a str, shell: &'a Shell) -> Vec<&'a OsStr> {
    let mut args = ["-t", host, "--"].map(OsStr::new).to_vec();
    if INHIBIT_IDLE_HOSTS.contains(&host) {
        args.extend(
            [
                "systemd-inhibit",
                "--who=tmux-attach-ssh",
                "--what=idle",
                "--mode=block",
            ]
            .map(OsStr::new),
        );
    }
    args.push(&shell.0);
    args.extend(["-lic", "tmux-attach"].map(OsStr::new));
    args
}

fn window_name(host: &str) -> String {
    format!("ssh {host}")
}

fn mark_window() -> Result<()> {
    tmux::run(["set-window-option", "@renamed", "on"]).map_err(anyhow::Error::msg)?;
    tmux::run(["set-window-option", "@nested_mode", "on"]).map_err(anyhow::Error::msg)?;
    Ok(())
}

fn exec_ssh(host: &str, shell: &Shell) -> Result<()> {
    Err(Command::new("ssh").args(ssh_args(host, shell)).exec()).context("running ssh")
}
