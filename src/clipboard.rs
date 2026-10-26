use std::io::Write;
use std::panic;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::tmux;
#[cfg(target_os = "linux")]
use crate::wayland;

// tmux asks the terminal for its clipboard and needs a moment to hear back.
const TMUX_REFRESH_WAIT: Duration = Duration::from_millis(100);

trait Backend: Sized {
    /// The backend, or why this session does not offer it.
    fn new() -> Result<Self>;
    fn copy(&self, data: &[u8]) -> Result<()>;
    fn paste(&self) -> Result<Vec<u8>>;
}

#[cfg(target_os = "android")]
struct Termux;

#[cfg(target_os = "android")]
impl Backend for Termux {
    fn new() -> Result<Self> {
        Ok(Self)
    }

    fn copy(&self, data: &[u8]) -> Result<()> {
        let mut command = Command::new("termux-clipboard-set");
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

    fn paste(&self) -> Result<Vec<u8>> {
        let mut command = Command::new("termux-clipboard-get");
        let output = command
            .stderr(Stdio::inherit())
            .output()
            .with_context(|| format!("running {command:?}"))?;
        if !output.status.success() {
            bail!("{command:?} failed: {}", output.status);
        }
        Ok(output.stdout)
    }
}

#[cfg(target_os = "linux")]
struct Wayland;

#[cfg(target_os = "linux")]
impl Backend for Wayland {
    fn new() -> Result<Self> {
        wayland::current_display().context("not running under Wayland")?;
        Ok(Self)
    }

    fn copy(&self, data: &[u8]) -> Result<()> {
        let mut command = Command::new("wl-copy");
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

    fn paste(&self) -> Result<Vec<u8>> {
        let mut command = Command::new("wl-paste");
        let output = command
            .stderr(Stdio::inherit())
            .output()
            .with_context(|| format!("running {command:?}"))?;
        if !output.status.success() {
            bail!("{command:?} failed: {}", output.status);
        }
        Ok(output.stdout)
    }
}

#[cfg(target_os = "macos")]
struct Macos;

#[cfg(target_os = "macos")]
impl Backend for Macos {
    fn new() -> Result<Self> {
        Ok(Self)
    }

    fn copy(&self, data: &[u8]) -> Result<()> {
        let mut command = Command::new("pbcopy");
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

    fn paste(&self) -> Result<Vec<u8>> {
        let mut command = Command::new("pbpaste");
        let output = command
            .stderr(Stdio::inherit())
            .output()
            .with_context(|| format!("running {command:?}"))?;
        if !output.status.success() {
            bail!("{command:?} failed: {}", output.status);
        }
        Ok(output.stdout)
    }
}

struct Tmux {
    socket: PathBuf,
}

impl Tmux {
    fn command<const N: usize>(&self, args: [&str; N]) -> Command {
        let mut command = tmux::command(["-S"]);
        command.arg(&self.socket).args(args);
        command
    }
}

impl Backend for Tmux {
    fn new() -> Result<Self> {
        let socket = tmux::current_socket()
            .map_err(anyhow::Error::msg)?
            .context("not running inside tmux")?;
        Ok(Self { socket })
    }

    fn copy(&self, data: &[u8]) -> Result<()> {
        let mut command = self.command(["load-buffer", "-w", "-"]);
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

    fn paste(&self) -> Result<Vec<u8>> {
        let mut refresh = self.command(["refresh-client", "-l"]);
        let status = refresh
            .status()
            .with_context(|| format!("running {refresh:?}"))
            .context("requesting the terminal's clipboard")?;
        if !status.success() {
            bail!("{refresh:?} failed: {status}");
        }
        thread::sleep(TMUX_REFRESH_WAIT);
        let mut command = self.command(["save-buffer", "-"]);
        let output = command
            .stderr(Stdio::inherit())
            .output()
            .with_context(|| format!("running {command:?}"))?;
        if !output.status.success() {
            bail!("{command:?} failed: {}", output.status);
        }
        Ok(output.stdout)
    }
}

macro_rules! clipboards {
    ($($(#[$attr:meta])* $backend:ident),* $(,)?) => {
        enum Clipboard {
            $($(#[$attr])* $backend($backend),)*
        }

        impl Clipboard {
            /// Every clipboard this platform has, in paste order, or why this session cannot use it.
            fn all() -> Vec<Result<Self>> {
                [
                    $($(#[$attr])* $backend::new().map(Self::$backend),)*
                ]
                .into_iter()
                .collect()
            }

            fn copy(&self, data: &[u8]) -> Result<()> {
                match self {
                    $($(#[$attr])* Self::$backend(backend) => backend.copy(data),)*
                }
            }

            fn paste(&self) -> Result<Vec<u8>> {
                match self {
                    $($(#[$attr])* Self::$backend(backend) => backend.paste(),)*
                }
            }
        }
    };
}

clipboards! {
    #[cfg(target_os = "android")]
    Termux,
    #[cfg(target_os = "linux")]
    Wayland,
    #[cfg(target_os = "macos")]
    Macos,
    Tmux,
}

pub fn paste() -> Result<Vec<u8>> {
    let mut failures = Vec::new();
    for clipboard in Clipboard::all() {
        match clipboard.and_then(|clipboard| clipboard.paste()) {
            Ok(data) => return Ok(data),
            Err(error) => failures.push(format!("{error:#}")),
        }
    }
    bail!("every clipboard command failed: {}", failures.join("; "))
}

/// Copies to every available clipboard at once and succeeds if any of them took the data.
pub fn copy(data: &[u8]) -> Result<()> {
    let results: Vec<Result<()>> = thread::scope(|scope| {
        let attempts: Vec<_> = Clipboard::all()
            .into_iter()
            .map(|clipboard| scope.spawn(move || clipboard?.copy(data)))
            .collect();
        attempts
            .into_iter()
            .map(|attempt| {
                attempt
                    .join()
                    .unwrap_or_else(|panic| panic::resume_unwind(panic))
            })
            .collect()
    });
    let mut copied = false;
    let mut failures = Vec::new();
    for result in results {
        match result {
            Ok(()) => copied = true,
            Err(error) => failures.push(format!("{error:#}")),
        }
    }
    if copied {
        return Ok(());
    }
    bail!("every clipboard command failed: {}", failures.join("; "))
}
