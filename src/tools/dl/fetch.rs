use std::io;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::tools::dl::filename::Tls;
use crate::tools::dl::target::Target;

const YT_DLP_OUTPUT_TEMPLATE: &str = "%(upload_date)s %(title)s [%(id)s].mkv";

pub fn direct(target: &Target, name: Option<&Path>, tls: &Tls) -> Result<()> {
    for mut command in [aria2c(target, name, tls), curl(target, name, tls)] {
        let program = command.get_program().display().to_string();
        match command.status() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => bail!("{program} failed: {status}"),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("running {program}")),
        }
    }
    bail!("aria2c and/or curl must be installed");
}

pub fn yt_dlp(target: &Target) -> Result<()> {
    let status = Command::new("yt-dlp")
        .args([
            "--cookies-from=firefox",
            "--output",
            YT_DLP_OUTPUT_TEMPLATE,
            "--convert-subtitles=srt",
            "--embed-subs",
            "--add-metadata",
            "--merge-output-format=mkv",
            "--",
        ])
        .arg(target.url().as_str())
        .status()
        .context("running yt-dlp")?;
    if !status.success() {
        bail!("yt-dlp failed: {status}");
    }
    Ok(())
}

fn aria2c(target: &Target, name: Option<&Path>, tls: &Tls) -> Command {
    let mut command = Command::new("aria2c");
    command.args([
        "--max-connection-per-server=16",
        "--max-concurrent-downloads=20",
        "--split=20",
        "--follow-torrent=false",
    ]);
    if let Tls::Unverified = tls {
        command.arg("--check-certificate=false");
    }
    if let Some(name) = name {
        command.arg("--out").arg(name);
    }
    command.arg("--").arg(target.url().as_str());
    command
}

fn curl(target: &Target, name: Option<&Path>, tls: &Tls) -> Command {
    let mut command = Command::new("curl");
    if let Tls::Unverified = tls {
        command.arg("--insecure");
    }
    command.arg("-L");
    match name {
        Some(name) => command.arg("-o").arg(name),
        None => command.arg("-O"),
    };
    command.arg("--").arg(target.url().as_str());
    command
}
