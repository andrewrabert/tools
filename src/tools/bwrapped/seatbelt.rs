//! The macOS backend: a seatbelt profile run with sandbox-exec.
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use tempfile::TempDir;

use super::paths;
use super::sandbox::{Defaults, Host, Sandbox};
use super::settings::Access;

/// Rights every process needs before it can reach a single file, and the traversal of every
/// parent directory on the way to an allowed path.
const PREAMBLE: &[&str] = &[
    "(version 1)",
    "(deny default)",
    "(allow process-exec* process-fork process-info* signal)",
    "(allow sysctl-read)",
    "(allow mach-lookup mach-task-name)",
    "(allow ipc-posix-shm)",
    "(allow file-read-metadata)",
    "(allow file-read* (literal \"/\"))",
    "(allow file-write-data (literal \"/dev/null\") (literal \"/dev/tty\"))",
    "(allow file-ioctl (literal \"/dev/dtracehelper\") (literal \"/dev/tty\"))",
    "(allow network*)",
];

pub struct Seatbelt {
    rules: Vec<Vec<u8>>,
    /// Emitted after every rule, so they win however broadly a directory was granted.
    denials: Vec<Vec<u8>>,
    env: BTreeMap<OsString, OsString>,
    cwd: Option<PathBuf>,
    trailer: Vec<OsString>,
    /// Where bubblewrap mounts an empty tmpfs over /tmp, seatbelt cannot replace a directory,
    /// so the sandbox gets a private directory of its own and the host /tmp stays unreachable.
    _scratch: TempDir,
}

/// A rule naming one fully resolved path. Seatbelt matches the real file, so a symlink is
/// stated as whatever it points at. A path that does not exist yet covers its whole tree, which
/// is what lets the sandbox create a directory the rule was written for.
fn rule(action: &str, operation: &str, path: &Path) -> Vec<u8> {
    let path = paths::resolve(path);
    let pattern = if path.is_file() { "literal" } else { "subpath" };
    let mut rule = format!("({action} {operation} ({pattern} \"").into_bytes();
    for &byte in path.as_os_str().as_bytes() {
        if byte == b'\\' || byte == b'"' {
            rule.push(b'\\');
        }
        rule.push(byte);
    }
    rule.extend_from_slice(b"\"))");
    rule
}

impl Sandbox for Seatbelt {
    const BIN: &'static str = "sandbox-exec";
    const REQUIRES_EXISTING_SOURCES: bool = false;

    fn new(_host: &Host) -> Result<Self> {
        let scratch = tempfile::Builder::new()
            .prefix("bwrapped-")
            .tempdir()
            .context("creating the sandbox's temporary directory")?;
        let scratch_path = fs::canonicalize(scratch.path())
            .context("resolving the sandbox's temporary directory")?;
        let mut sandbox = Seatbelt {
            rules: PREAMBLE
                .iter()
                .map(|rule| rule.as_bytes().to_vec())
                .collect(),
            denials: Vec::new(),
            env: env::vars_os().collect(),
            cwd: None,
            trailer: Vec::new(),
            _scratch: scratch,
        };
        sandbox.mount(&scratch_path, &scratch_path, Access::ReadWrite)?;
        sandbox.setenv(OsStr::new("TMPDIR"), scratch_path.as_os_str());
        Ok(sandbox)
    }

    fn defaults(host: &Host) -> Defaults {
        let mut ro = host.shared_ro();
        ro.extend(
            [
                "/Applications",
                "/Library",
                "/System",
                "/private/etc",
                "/private/var/db",
                "/private/var/select",
                "/sbin",
            ]
            .map(PathBuf::from),
        );
        Defaults {
            ro,
            rw: vec!["/dev".into(), "/tmp".into(), host.work_dir.clone()],
            inherit: Vec::new(),
            set: vec![
                ("HOME", host.home.clone().into_os_string()),
                // seatbelt cannot fake an empty file at the client's global config path;
                // denial reads as EPERM, which noted treats as fatal
                ("NOTED_ENV_FILE", "/dev/null".into()),
            ],
        }
    }

    // Seatbelt cannot remap a path, so a destination that renames its source cannot be honored.
    // The write denial sits beside its own allow rather than with the trailing denials, so a
    // later read-write rule for a path underneath still wins.
    fn mount(&mut self, source: &Path, destination: &Path, access: Access) -> Result<()> {
        if destination != source {
            bail!(
                "seatbelt cannot mount {} at {}: destinations must match their source",
                source.display(),
                destination.display()
            );
        }
        match access {
            Access::ReadOnly => {
                self.rules.push(rule("allow", "file-read*", source));
                self.rules.push(rule("deny", "file-write*", source));
            }
            Access::ReadWrite => self.rules.push(rule(
                "allow",
                "file* file-read* file-write* file-ioctl",
                source,
            )),
        }
        Ok(())
    }

    fn hide(&mut self, path: &Path) {
        self.denials.push(rule("deny", "file-read*", path));
    }

    fn exclude(&mut self, path: &Path) {
        self.hide(path);
    }

    fn setenv(&mut self, name: &OsStr, value: &OsStr) {
        self.env.insert(name.to_owned(), value.to_owned());
    }

    fn unsetenv(&mut self, name: &OsStr) {
        self.env.remove(name);
    }

    fn chdir(&mut self, path: &Path) {
        self.cwd = Some(path.to_owned());
    }

    fn arg(&mut self, arg: &OsStr) {
        self.trailer.push(arg.to_owned());
    }

    fn command(&self) -> Command {
        let mut profile = Vec::new();
        for rule in self.rules.iter().chain(&self.denials) {
            profile.extend_from_slice(rule);
            profile.push(b'\n');
        }
        let mut command = Command::new(Self::BIN);
        command
            .arg("-p")
            .arg(OsString::from_vec(profile))
            .args(&self.trailer)
            .env_clear()
            .envs(&self.env);
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_quote_paths_and_match_files_literally() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let file = root.join("a\"b");
        fs::write(&file, "").unwrap();
        assert_eq!(
            String::from_utf8(rule("allow", "file-read*", &file)).unwrap(),
            format!("(allow file-read* (literal \"{}/a\\\"b\"))", root.display())
        );
        assert_eq!(
            String::from_utf8(rule("deny", "file-read*", &root)).unwrap(),
            format!("(deny file-read* (subpath \"{}\"))", root.display())
        );
    }

    #[test]
    fn renaming_mounts_are_refused() {
        let host = Host {
            home: "/home/u".into(),
            work_dir: "/work".into(),
            uid: 501,
        };
        let mut sandbox = Seatbelt::new(&host).unwrap();
        assert!(
            sandbox
                .mount(Path::new("/a"), Path::new("/b"), Access::ReadOnly)
                .is_err()
        );
    }

    #[test]
    fn denials_follow_every_rule() {
        let host = Host {
            home: "/home/u".into(),
            work_dir: "/work".into(),
            uid: 501,
        };
        let mut sandbox = Seatbelt::new(&host).unwrap();
        sandbox.hide(Path::new("/nonexistent/secret"));
        sandbox
            .mount(
                Path::new("/nonexistent/dir"),
                Path::new("/nonexistent/dir"),
                Access::ReadWrite,
            )
            .unwrap();
        let command = sandbox.command();
        let profile = command.get_args().nth(1).unwrap().to_str().unwrap();
        let lines: Vec<&str> = profile.lines().collect();
        assert_eq!(lines[0], "(version 1)");
        assert_eq!(
            lines.last().unwrap(),
            &"(deny file-read* (subpath \"/nonexistent/secret\"))"
        );
    }
}
