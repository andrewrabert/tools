//! The Linux backend: bubblewrap.
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;

use super::paths;
use super::sandbox::{Defaults, Host, Sandbox};
use super::settings::{Access, Bind};

pub struct Bubblewrap {
    args: Vec<OsString>,
    /// (target, link) pairs bwrap recreates after the mounts.
    symlinks: Vec<(PathBuf, PathBuf)>,
    chdir: Option<PathBuf>,
    trailer: Vec<OsString>,
}

impl Bubblewrap {
    fn push<S: AsRef<OsStr>>(&mut self, args: impl IntoIterator<Item = S>) {
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().to_owned()));
    }
}

impl Sandbox for Bubblewrap {
    const BIN: &'static str = "bwrap";
    const REQUIRES_EXISTING_SOURCES: bool = true;

    fn new(host: &Host) -> Result<Self> {
        let mut sandbox = Bubblewrap {
            args: Vec::new(),
            symlinks: Vec::new(),
            chdir: None,
            trailer: Vec::new(),
        };
        sandbox.push(["--proc", "/proc"]);
        sandbox.push(["--dev", "/dev"]);
        sandbox.push(["--tmpfs", "/tmp"]);
        sandbox.push(["--tmpfs", "/run"]);
        sandbox.push(["--dir".to_owned(), format!("/run/user/{}", host.uid)]);
        Ok(sandbox)
    }

    fn defaults(host: &Host) -> Defaults {
        let mut ro = host.shared_ro();
        ro.extend(
            [
                "/etc",
                "/lib",
                "/lib64",
                "/run/systemd/resolve",
                "/run/log/journal",
                "/var/log/journal",
                "/var/lib/systemd/coredump",
            ]
            .map(PathBuf::from),
        );
        Defaults {
            ro,
            rw: vec![host.work_dir.clone()],
            inherit: vec!["PATH", "TERM", "USER"],
            set: vec![
                ("HOME", host.home.clone().into_os_string()),
                ("TMPDIR", "/tmp".into()),
            ],
        }
    }

    fn default_bind(&mut self, path: &Path, access: Access) -> Option<Bind> {
        if !path.exists() {
            return None;
        }
        // bwrap cannot mount onto a symlink; bind the target and recreate the link.
        let source = if path.is_symlink() {
            let target = paths::resolve(path);
            self.symlinks.push((target.clone(), path.to_owned()));
            target
        } else {
            path.to_owned()
        };
        Some(Bind {
            source,
            target: None,
            access,
        })
    }

    fn finish_mounts(&mut self) {
        for (target, link) in std::mem::take(&mut self.symlinks) {
            self.push([
                OsStr::new("--symlink"),
                target.as_os_str(),
                link.as_os_str(),
            ]);
        }
    }

    fn mount(&mut self, source: &Path, destination: &Path, access: Access) -> Result<()> {
        let flag = match access {
            Access::ReadOnly => "--ro-bind",
            Access::ReadWrite => "--bind",
        };
        self.push([
            OsStr::new(flag),
            source.as_os_str(),
            destination.as_os_str(),
        ]);
        Ok(())
    }

    fn hide(&mut self, path: &Path) {
        self.push([
            OsStr::new("--ro-bind"),
            OsStr::new("/dev/null"),
            path.as_os_str(),
        ]);
    }

    fn exclude(&mut self, path: &Path) {
        if path.is_dir() {
            self.push([OsStr::new("--tmpfs"), path.as_os_str()]);
        } else {
            self.push([
                OsStr::new("--bind"),
                OsStr::new("/dev/null"),
                path.as_os_str(),
            ]);
        }
    }

    fn setenv(&mut self, name: &OsStr, value: &OsStr) {
        self.push([OsStr::new("--setenv"), name, value]);
    }

    fn unsetenv(&mut self, name: &OsStr) {
        self.push([OsStr::new("--unsetenv"), name]);
    }

    fn chdir(&mut self, path: &Path) {
        self.chdir = Some(path.to_owned());
    }

    fn arg(&mut self, arg: &OsStr) {
        self.trailer.push(arg.to_owned());
    }

    fn command(&self) -> Command {
        let mut command = Command::new(Self::BIN);
        command.args(&self.args);
        if let Some(chdir) = &self.chdir {
            command.arg("--chdir").arg(chdir);
        }
        command
            .args(["--unshare-pid", "--die-with-parent"])
            .args(&self.trailer);
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> Host {
        Host {
            home: "/home/u".into(),
            work_dir: "/work".into(),
            uid: 1000,
        }
    }

    fn args(command: &Command) -> Vec<&str> {
        command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect()
    }

    #[test]
    fn command_places_options_before_the_program() {
        let mut sandbox = Bubblewrap::new(&host()).unwrap();
        sandbox
            .mount(Path::new("/a"), Path::new("/b"), Access::ReadOnly)
            .unwrap();
        sandbox.hide(Path::new("/secret"));
        sandbox.setenv(OsStr::new("K"), OsStr::new("V"));
        sandbox.unsetenv(OsStr::new("U"));
        sandbox.chdir(Path::new("/work"));
        sandbox.arg(OsStr::new("/usr/bin/prog"));
        sandbox.arg(OsStr::new("--flag"));
        assert_eq!(
            args(&sandbox.command()),
            [
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "--tmpfs",
                "/tmp",
                "--tmpfs",
                "/run",
                "--dir",
                "/run/user/1000",
                "--ro-bind",
                "/a",
                "/b",
                "--ro-bind",
                "/dev/null",
                "/secret",
                "--setenv",
                "K",
                "V",
                "--unsetenv",
                "U",
                "--chdir",
                "/work",
                "--unshare-pid",
                "--die-with-parent",
                "/usr/bin/prog",
                "--flag",
            ]
        );
    }

    #[test]
    fn symlinked_defaults_bind_their_target_and_recreate_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir(root.join("real")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();

        let mut sandbox = Bubblewrap::new(&host()).unwrap();
        let bind = sandbox
            .default_bind(&root.join("link"), Access::ReadOnly)
            .unwrap();
        assert_eq!(bind.source, root.join("real"));
        assert!(
            sandbox
                .default_bind(&root.join("missing"), Access::ReadOnly)
                .is_none()
        );
        sandbox.finish_mounts();
        let link = root.join("link");
        let real = root.join("real");
        assert!(
            args(&sandbox.command())
                .windows(3)
                .any(|window| window
                    == ["--symlink", real.to_str().unwrap(), link.to_str().unwrap()])
        );
    }
}
