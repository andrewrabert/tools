use std::env;
use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

const LF_COPY_SUFFIX: &str = ".~1~";
const DEFAULT_DESKTOP: &str = "kde";
const DEFAULT_KDE_SESSION_VERSION: &str = "6";

pub struct MimeType(String);

impl MimeType {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn detect(file: &Path) -> Result<Self> {
        let confuses_xdg_mime = file.to_string_lossy().ends_with(LF_COPY_SUFFIX);
        if !confuses_xdg_mime {
            match xdg_mime(file).stderr(Stdio::inherit()).output() {
                Ok(output) if output.status.success() => {
                    return Ok(MimeType::from_output(&output.stdout));
                }
                Ok(output) => bail!("xdg-mime failed: {}", output.status),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).context("running xdg-mime"),
            }
        }
        let output = Command::new("file")
            .args(["--brief", "--mime-type"])
            .arg(file)
            .stderr(Stdio::inherit())
            .output()
            .context("running file")?;
        if !output.status.success() {
            bail!("file failed: {}", output.status);
        }
        Ok(MimeType::from_output(&output.stdout))
    }

    fn from_output(output: &[u8]) -> Self {
        MimeType(String::from_utf8_lossy(output).trim().to_owned())
    }
}

fn xdg_mime(file: &Path) -> Command {
    let desktop = non_empty_var("XDG_CURRENT_DESKTOP").unwrap_or_else(|| DEFAULT_DESKTOP.into());
    let mut command = Command::new("xdg-mime");
    command.args(["query", "filetype"]).arg(file);
    if desktop.eq_ignore_ascii_case(DEFAULT_DESKTOP) {
        let version = non_empty_var("KDE_SESSION_VERSION")
            .unwrap_or_else(|| DEFAULT_KDE_SESSION_VERSION.into());
        command.env("KDE_SESSION_VERSION", version);
    }
    command.env("XDG_CURRENT_DESKTOP", desktop);
    command
}

fn non_empty_var(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}
