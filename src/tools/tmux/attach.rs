use std::env;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{ExitCode, Stdio};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::dirs;
use crate::tmux;
use crate::tools::Tool;

const GUI_VARIABLES: [&str; 3] = ["WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "DISPLAY"];

#[derive(ClapArgs)]
#[command(about = "Attach to the first tmux session, detaching other clients, or start one")]
pub struct Attach {
    #[arg(long, help = "wait for enter before taking over an attached session")]
    pub prompt: bool,
    #[arg(long, help = "fail instead of taking over an attached session")]
    pub no_detach: bool,
}

impl Tool for Attach {
    fn run(self) -> ExitCode {
        run(self).unwrap_or_else(|error| {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        })
    }
}

enum FirstSession {
    Missing,
    Detached,
    Attached,
}

impl FirstSession {
    fn query() -> Self {
        let output = tmux::command(["list-sessions"])
            .stderr(Stdio::null())
            .output();
        let listing = output
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .unwrap_or_default();
        match listing.lines().next() {
            None | Some("") => FirstSession::Missing,
            Some(line) if line.ends_with("(attached)") => FirstSession::Attached,
            Some(_) => FirstSession::Detached,
        }
    }
}

fn run(args: Attach) -> Result<ExitCode> {
    let home = dirs::home()?;
    env::set_current_dir(&home).with_context(|| format!("entering {}", home.display()))?;

    let session = FirstSession::query();
    if let FirstSession::Attached = session {
        if args.no_detach {
            return Ok(ExitCode::FAILURE);
        }
        if args.prompt {
            println!("Press enter to attach to tmux session ...");
            io::stdin()
                .read_line(&mut String::new())
                .context("reading stdin")?;
        }
    }
    let error = match session {
        FirstSession::Missing => tmux::command(["new-session"]).exec(),
        FirstSession::Detached | FirstSession::Attached => {
            update_gui_environment();
            tmux::command(["attach", "-d"]).exec()
        }
    };
    Err(error).context("running tmux")
}

fn update_gui_environment() {
    let has_display = ["WAYLAND_DISPLAY", "DISPLAY"]
        .into_iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()));
    if !has_display {
        return;
    }
    for name in GUI_VARIABLES {
        let value = env::var_os(name).unwrap_or_default();
        let _ = tmux::command(["set-environment", "-g", name])
            .arg(value)
            .stderr(Stdio::null())
            .status();
    }
}
