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

    fn enabled(self) -> bool {
        match self {
            Self::Termux | Self::Macos => true,
            Self::Wayland => is_set("WAYLAND_DISPLAY"),
            Self::Tmux => tmux::current_socket().is_ok(),
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
        // A failed refresh only means a stale buffer; save-buffer decides.
        let _ = tmux::command(["refresh-client", "-l"]).status();
        thread::sleep(TMUX_REFRESH_WAIT);
    }
    let output = backend.paste().stderr(Stdio::inherit()).output()?;
    if !output.status.success() {
        bail!("{}", output.status);
    }
    Ok(output.stdout)
}

fn try_copy(backend: Backend, data: &[u8]) -> Result<()> {
    let mut child = backend.copy().stdin(Stdio::piped()).spawn()?;
    // Dropping stdin closes the pipe so the command sees end of input.
    let mut stdin = child.stdin.take().context("opening stdin")?;
    let written = stdin.write_all(data);
    drop(stdin);
    let status = child.wait()?;
    written?;
    if !status.success() {
        bail!("{status}");
    }
    Ok(())
}

// Any failure moves on to the next backend, as the shell version did.
pub fn paste() -> Result<Vec<u8>> {
    Backend::ALL
        .into_iter()
        .filter(|backend| backend.enabled())
        .find_map(|backend| try_paste(backend).ok())
        .context("no suitable clipboard command detected")
}

pub fn copy(data: &[u8]) -> Result<()> {
    Backend::ALL
        .into_iter()
        .filter(|backend| backend.enabled())
        .find_map(|backend| try_copy(backend, data).ok())
        .context("no suitable clipboard command detected")
}
