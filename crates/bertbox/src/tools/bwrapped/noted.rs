//! The `noted server socket` process one invocation owns.
use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{self, Child, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

const SCHEME: &[u8] = b"unix://";
const ENDPOINT_TIMEOUT: Duration = Duration::from_secs(10);
const SERVER_GRACE: Duration = Duration::from_secs(5);
const SERVER_POLL: Duration = Duration::from_millis(50);
/// The working directory of the server, so no `.notedenv` at or above the invocation directory
/// is read and the root comes from the wrapper's own NOTED_DIR or noted.env.
const SERVER_CWD: &str = "/";

/// The `unix://<path>` URL the server announced.
#[derive(Debug, PartialEq, Eq)]
pub struct Endpoint(OsString);

impl Endpoint {
    fn parse(line: &[u8]) -> Result<Endpoint> {
        if line.len() <= SCHEME.len() || !line.starts_with(SCHEME) {
            bail!(
                "noted server socket announced {:?}",
                String::from_utf8_lossy(line)
            );
        }
        Ok(Endpoint(OsString::from_vec(line.to_vec())))
    }

    pub fn url(&self) -> &OsStr {
        &self.0
    }

    pub fn socket(&self) -> &Path {
        Path::new(OsStr::from_bytes(&self.0.as_bytes()[SCHEME.len()..]))
    }
}

/// A running server, terminated when dropped.
pub struct NotesServer {
    child: Child,
    // Held open so the server never writes to a closed pipe.
    _stdout: ChildStdout,
    endpoint: Endpoint,
}

impl NotesServer {
    /// Starts the server in its own session with the wrapper's environment minus NOTED_URL, and
    /// waits for the endpoint it announces on stdout. Where the platform can arrange it, the
    /// server dies with the wrapper.
    pub fn start(scope: &str) -> Result<NotesServer> {
        let mut command = Command::new("noted");
        command
            .args(["server", "socket"])
            .arg(format!("--scope={scope}"))
            .current_dir(SERVER_CWD)
            .env_remove("NOTED_URL")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let parent = process::id();
        // SAFETY: `detach` only makes async-signal-safe calls.
        unsafe { command.pre_exec(move || detach(parent)) };
        let mut child = command.spawn().context("cannot run noted")?;
        let stdout = child.stdout.take().context("noted has no stdout")?;
        match read_endpoint(stdout) {
            Ok((stdout, endpoint)) => Ok(NotesServer {
                child,
                _stdout: stdout,
                endpoint,
            }),
            Err(error) => {
                terminate(&mut child);
                Err(error)
            }
        }
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}

impl Drop for NotesServer {
    fn drop(&mut self) {
        terminate(&mut self.child);
    }
}

/// Runs in the forked child: a new session, so no terminal-generated signal reaches the server,
/// and on Linux a SIGKILL once `parent` dies, exiting at once if that death beat the prctl.
fn detach(parent: u32) -> io::Result<()> {
    // SAFETY: setsid takes no arguments and only affects this process.
    if unsafe { libc::setsid() } == -1 {
        return Err(io::Error::last_os_error());
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    // SAFETY: prctl, getppid, and _exit are async-signal-safe.
    unsafe {
        libc::prctl(
            libc::PR_SET_PDEATHSIG,
            libc::SIGKILL as libc::c_ulong,
            0,
            0,
            0,
        );
        if libc::getppid() as u32 != parent {
            libc::_exit(1);
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let _ = parent;
    Ok(())
}

/// The first line of `stdout`, within ENDPOINT_TIMEOUT. A timeout, an EOF, and a line that is
/// not `unix://<path>` are rejected.
fn read_endpoint(stdout: ChildStdout) -> Result<(ChildStdout, Endpoint)> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut stdout = stdout;
        let line = first_line(&mut stdout);
        let _ = sender.send((stdout, line));
    });
    match receiver.recv_timeout(ENDPOINT_TIMEOUT) {
        Ok((stdout, line)) => Ok((stdout, Endpoint::parse(&line?)?)),
        Err(_) => bail!(
            "noted server socket announced no endpoint within {}s",
            ENDPOINT_TIMEOUT.as_secs()
        ),
    }
}

fn first_line(reader: &mut impl Read) -> Result<Vec<u8>> {
    let mut buffered = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        if let Some(end) = buffered.iter().position(|&byte| byte == b'\n') {
            buffered.truncate(end);
            return Ok(buffered);
        }
        let read = reader
            .read(&mut chunk)
            .context("reading the noted server's endpoint")?;
        if read == 0 {
            bail!("noted server socket closed stdout before announcing an endpoint");
        }
        buffered.extend_from_slice(&chunk[..read]);
    }
}

/// SIGTERM, then SIGKILL SERVER_GRACE later; returns once the child is reaped.
fn terminate(child: &mut Child) {
    if !matches!(child.try_wait(), Ok(None)) {
        return;
    }
    if let Ok(pid) = i32::try_from(child.id()) {
        let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
    }
    let deadline = Instant::now() + SERVER_GRACE;
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        thread::sleep(SERVER_POLL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_must_name_a_unix_socket() {
        let endpoint = Endpoint::parse(b"unix:///run/noted.sock").unwrap();
        assert_eq!(endpoint.url(), "unix:///run/noted.sock");
        assert_eq!(endpoint.socket(), Path::new("/run/noted.sock"));
        assert!(Endpoint::parse(b"unix://").is_err());
        assert!(Endpoint::parse(b"http://localhost").is_err());
    }

    #[test]
    fn first_line_stops_at_newline() {
        assert_eq!(first_line(&mut &b"one\ntwo\n"[..]).unwrap(), b"one");
        assert!(first_line(&mut &b"partial"[..]).is_err());
    }
}
