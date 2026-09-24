use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use nix::unistd;

pub const VARIABLE: &str = "HOST_DOTFILES";
const PIKVM: &str = "pikvm";
const ZSH_OVERRIDE: &str =
    r#"if [ -f ~/.zshenv.local ]; then . ~/.zshenv.local; fi && echo "$HOST_DOTFILES""#;

pub enum Host {
    Pikvm,
    Other(OsString),
}

impl Host {
    pub fn resolve() -> Result<Self> {
        let mut name = unistd::gethostname().context("reading the hostname")?;
        if let Some(output) = capture_if_installed(
            Command::new("zsh")
                .args(["-lic", ZSH_OVERRIDE])
                .env(VARIABLE, &name),
        )? {
            name = trimmed(output);
        }
        if let Some(output) = capture_if_installed(Command::new("hostnamectl").arg("hostname"))? {
            name = trimmed(output);
        }
        if name.is_empty() {
            bail!("cannot determine hostname");
        }
        Ok(if name == PIKVM {
            Host::Pikvm
        } else {
            Host::Other(name)
        })
    }

    pub fn name(&self) -> OsString {
        match self {
            Host::Pikvm => PIKVM.into(),
            Host::Other(name) => name.clone(),
        }
    }
}

fn capture_if_installed(command: &mut Command) -> Result<Option<Vec<u8>>> {
    let program = command.get_program().display().to_string();
    match command.stderr(Stdio::inherit()).output() {
        Ok(output) if output.status.success() => Ok(Some(output.stdout)),
        Ok(output) => bail!("{program} failed: {}", output.status),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("running {program}")),
    }
}

fn trimmed(output: Vec<u8>) -> OsString {
    OsString::from_vec(output.trim_ascii().to_vec())
}
