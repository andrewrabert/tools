use std::env;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::{self, Command, Stdio};

pub fn current_socket() -> Result<PathBuf, &'static str> {
    let value = env::var_os("TMUX").ok_or("TMUX is not set")?;
    let socket = value
        .as_bytes()
        .split(|&byte| byte == b',')
        .next()
        .filter(|path| !path.is_empty())
        .ok_or("TMUX has no socket path")?;
    Ok(PathBuf::from(OsStr::from_bytes(socket)))
}

pub fn command<I, S>(args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("tmux");
    command.args(args);
    command
}

pub fn new_server<I, S>(args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = command(["-L"]);
    command.arg(format!("bertbox-{}", process::id())).args(args);
    command
}

pub fn run<I, S>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = command(args);
    let status = command
        .status()
        .map_err(|error| format!("running {command:?}: {error}"))?;
    if !status.success() {
        return Err(format!("{command:?} failed: {status}"));
    }
    Ok(())
}

pub fn capture<I, S>(args: I) -> Result<Vec<u8>, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = command(args);
    let output = command
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|child| child.wait_with_output())
        .map_err(|error| format!("running {command:?}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{command:?} failed: {}", output.status));
    }
    Ok(output.stdout)
}
