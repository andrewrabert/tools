use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

use anyhow::{Context, Result};
use nix::errno::Errno;
use nix::sys::signal;
use nix::sys::stat::Mode;
use nix::unistd::{self, Pid};

use crate::tools::fanpipe::topic::Topic;

const PREFIX: &str = "fanpipe-";

struct PipeName {
    topic: String,
    pid: Pid,
}

impl PipeName {
    fn parse(name: &OsStr) -> Option<Self> {
        let (topic, pid) = name.to_str()?.strip_prefix(PREFIX)?.rsplit_once('-')?;
        Some(PipeName {
            topic: topic.to_owned(),
            pid: Pid::from_raw(pid.parse().ok()?),
        })
    }

    fn owner_is_gone(&self) -> bool {
        signal::kill(self.pid, None) == Err(Errno::ESRCH)
    }
}

pub struct PipeDir(PathBuf);

impl PipeDir {
    pub fn open() -> Self {
        PipeDir(env::temp_dir())
    }

    pub fn sweep(&self) -> Result<()> {
        for (path, name) in self.pipes()? {
            if name.owner_is_gone() {
                remove(&path)?;
            }
        }
        Ok(())
    }

    pub fn subscribers(&self, topic: &Topic) -> Result<Vec<PathBuf>> {
        let topic = topic.to_string();
        Ok(self
            .pipes()?
            .into_iter()
            .filter(|(_, name)| name.topic == topic)
            .map(|(path, _)| path)
            .collect())
    }

    pub fn create(&self, topic: &Topic) -> Result<OwnedPipe> {
        let path = self.0.join(format!("{PREFIX}{topic}-{}", process::id()));
        unistd::mkfifo(&path, Mode::from_bits_truncate(0o666))
            .with_context(|| format!("creating {}", path.display()))?;
        Ok(OwnedPipe(path))
    }

    fn pipes(&self) -> Result<Vec<(PathBuf, PipeName)>> {
        let entries =
            fs::read_dir(&self.0).with_context(|| format!("reading {}", self.0.display()))?;
        let mut pipes = Vec::new();
        for entry in entries {
            let entry = entry.with_context(|| format!("reading {}", self.0.display()))?;
            if let Some(name) = PipeName::parse(&entry.file_name()) {
                pipes.push((entry.path(), name));
            }
        }
        Ok(pipes)
    }
}

pub struct OwnedPipe(PathBuf);

impl OwnedPipe {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for OwnedPipe {
    fn drop(&mut self) {
        if let Err(e) = remove(&self.0) {
            eprintln!("{e:#}");
        }
    }
}

fn remove(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}
