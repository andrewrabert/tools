use std::env;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::tmux;

// tmux asks the terminal for its clipboard and needs a moment to hear back.
const TMUX_REFRESH_WAIT: Duration = Duration::from_millis(100);

#[derive(Clone, Copy)]
enum Backend {
    Termux,
    Wayland,
    Macos,
    Tmux,
}

impl Backend {
    const ALL: [Self; 4] = [Self::Termux, Self::Wayland, Self::Macos, Self::Tmux];

    fn enabled(self) -> Result<bool> {
        match self {
            Self::Termux | Self::Macos => Ok(true),
            Self::Wayland => Ok(is_set("WAYLAND_DISPLAY")),
            Self::Tmux => Ok(tmux::current_socket()
                .map_err(anyhow::Error::msg)?
                .is_some()),
        }
    }

    fn copy(self) -> Command {
        match self {
            Self::Termux => Command::new("termux-clipboard-set"),
            Self::Wayland => Command::new("wl-copy"),
            Self::Macos => Command::new("pbcopy"),
            Self::Tmux => tmux::command(["load-buffer", "-w", "-"]),
        }
    }

    fn paste(self) -> Command {
        match self {
            Self::Termux => Command::new("termux-clipboard-get"),
            Self::Wayland => Command::new("wl-paste"),
            Self::Macos => Command::new("pbpaste"),
            Self::Tmux => tmux::command(["save-buffer", "-"]),
        }
    }
}

fn is_set(variable: &str) -> bool {
    env::var_os(variable).is_some_and(|value| !value.is_empty())
}

fn try_paste(backend: Backend) -> Result<Vec<u8>> {
    if let Backend::Tmux = backend {
        tmux::run(["refresh-client", "-l"])
            .map_err(anyhow::Error::msg)
            .context("requesting the terminal's clipboard")?;
        thread::sleep(TMUX_REFRESH_WAIT);
    }
    let mut command = backend.paste();
    let output = command
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("running {command:?}"))?;
    if !output.status.success() {
        bail!("{command:?} failed: {}", output.status);
    }
    Ok(output.stdout)
}

fn try_copy(backend: Backend, data: &[u8]) -> Result<()> {
    let mut command = backend.copy();
    let mut child = command
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {command:?}"))?;
    // Dropping stdin closes the pipe so the command sees end of input.
    let mut stdin = child.stdin.take().context("opening stdin")?;
    let written = stdin.write_all(data);
    drop(stdin);
    let status = child
        .wait()
        .with_context(|| format!("waiting for {command:?}"))?;
    written.with_context(|| format!("writing to {command:?}"))?;
    if !status.success() {
        bail!("{command:?} failed: {status}");
    }
    Ok(())
}

fn first_success<T>(mut attempt: impl FnMut(Backend) -> Result<T>) -> Result<T> {
    let mut failures = Vec::new();
    for backend in Backend::ALL {
        let result = match backend.enabled() {
            Ok(false) => continue,
            Ok(true) => attempt(backend),
            Err(error) => Err(error),
        };
        match result {
            Ok(value) => return Ok(value),
            Err(error) => failures.push(format!("{error:#}")),
        }
    }
    if failures.is_empty() {
        bail!("no suitable clipboard command detected");
    }
    bail!("every clipboard command failed: {}", failures.join("; "))
}

pub fn paste() -> Result<Vec<u8>> {
    first_success(try_paste)
}

pub fn copy(data: &[u8]) -> Result<()> {
    first_success(|backend| try_copy(backend, data))
}
