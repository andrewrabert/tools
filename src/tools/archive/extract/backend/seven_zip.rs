use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use regex::Regex;

use crate::tools::archive::extract::backend::{Destination, Request};
use crate::tools::archive::extract::config::Overwrite;
use crate::tools::archive::extract::listing::{self, Contents, Entry};
use crate::tools::archive::password::Password;
use crate::tools::archive::process;

// Termux names the official 7-Zip `7zz`, and `7z` there is p7zip.
const PROGRAMS: [&str; 2] = ["7zz", "7z"];
const FIELD_SEPARATOR: &[u8] = b" = ";
const VOLUMES_FIELD: &str = "Volumes = ";
const MODIFIED_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

static RAR_PART: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*\.part)(\d+)(\.rar)$").expect("valid regex"));
static NUMBERED_PART: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*\.)(\d+)()$").expect("valid regex"));

pub fn program() -> Result<&'static str> {
    PROGRAMS
        .into_iter()
        .find(|program| process::installed(program))
        .context("neither 7zz nor 7z is installed")
}

fn password_flag(password: Option<&Password>) -> String {
    // An empty password keeps 7z from prompting for one.
    format!("-p{}", password.map_or("", Password::as_str))
}

pub fn extract(request: &Request) -> Result<()> {
    let mut command = Command::new(program()?);
    command.args(["-bd", "-bb0"]);
    if let Overwrite::Replace = request.overwrite {
        command.arg("-aoa");
    }
    match request.destination {
        Destination::Stream(fd) => {
            let output = fd.try_clone().context("duplicating the output stream")?;
            command
                .arg("e")
                .arg(password_flag(request.password))
                .args(["-spd", "-so"])
                .stdout(output);
        }
        Destination::Directory(directory) => {
            command
                .arg("x")
                .arg(password_flag(request.password))
                .args(["-spe", "-spd"])
                .arg(process::prefixed("-o", directory.as_os_str()))
                // 7z reports every archive even at its quietest log level
                .stdout(Stdio::null());
        }
    }
    process::run(
        command
            .arg("--")
            .arg(request.archive)
            .args(request.members)
            // 7z asks what to do about files that already exist
            .stdin(Stdio::null()),
    )
}

#[derive(Default)]
struct Block {
    path: Option<Vec<u8>>,
    is_dir: bool,
    entry: Entry,
}

pub fn contents(archive: &Path) -> Result<Contents> {
    let output = process::capture(
        Command::new(program()?)
            .args(["-ba", "l", "-slt", "--"])
            .arg(archive),
    )?;

    let mut contents = Contents::default();
    let mut block = Block::default();
    let mut lines = listing::lines(&output);
    lines.push(b"");
    for line in lines {
        if line.is_empty() {
            let Block {
                path,
                is_dir,
                entry,
            } = std::mem::take(&mut block);
            if let (Some(path), false) = (path, is_dir) {
                contents.insert(path, entry);
            }
            continue;
        }
        let separator = line
            .windows(FIELD_SEPARATOR.len())
            .position(|window| window == FIELD_SEPARATOR)
            .with_context(|| format!("unexpected 7z output: {}", String::from_utf8_lossy(line)))?;
        let value = &line[separator + FIELD_SEPARATOR.len()..];
        match &line[..separator] {
            b"Path" => block.path = Some(value.to_vec()),
            // zip archives mark directories with Folder rather than Attributes
            b"Folder" => block.is_dir |= value == b"+",
            b"Attributes" => {
                block.is_dir |= listing::split_fields(value, 1).first() == Some(&b"D".as_slice())
            }
            // .dmg entries have an empty Size
            b"Size" if !value.is_empty() => {
                let size = std::str::from_utf8(value)
                    .ok()
                    .and_then(|size| size.parse::<u64>().ok())
                    .with_context(|| {
                        format!("unexpected 7z size: {}", String::from_utf8_lossy(value))
                    })?;
                block.entry.size = Some(size);
            }
            b"Modified" if !value.is_empty() => {
                let modified =
                    std::str::from_utf8(value).context("unexpected 7z modification time")?;
                block.entry.mtime = Some(mtime(modified)?);
            }
            _ => {}
        }
    }
    Ok(contents)
}

// The fraction's leading zeros are dropped: file-hash-recorder compares listings against
// databases that hold mtimes computed this way.
fn mtime(modified: &str) -> Result<f64> {
    let (seconds, fraction) = modified.split_once('.').unwrap_or((modified, "0"));
    let seconds = DateTime::strptime(MODIFIED_FORMAT, seconds)
        .and_then(|local| local.to_zoned(TimeZone::system()))
        .with_context(|| format!("unexpected 7z modification time: {modified}"))?
        .timestamp()
        .as_second();
    let fraction: u64 = fraction
        .parse()
        .with_context(|| format!("unexpected 7z modification time: {modified}"))?;
    format!("{seconds}.{fraction}")
        .parse()
        .with_context(|| format!("unexpected 7z modification time: {modified}"))
}

pub fn volumes(archive: &Path, password: Option<&Password>) -> Result<Vec<PathBuf>> {
    let output = process::capture(
        Command::new(program()?)
            .arg("l")
            .arg(password_flag(password))
            .arg("--")
            .arg(archive),
    )?;
    let output = String::from_utf8_lossy(&output);
    let count: usize = match output
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(VOLUMES_FIELD))
    {
        Some(count) => count
            .trim()
            .parse()
            .with_context(|| format!("unexpected 7z volume count: {count}"))?,
        None => 1,
    };
    if count <= 1 {
        return Ok(vec![archive.to_owned()]);
    }

    let name = archive
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", archive.display()))?;
    let Some(parts) = RAR_PART
        .captures(name)
        .or_else(|| NUMBERED_PART.captures(name))
    else {
        bail!("cannot tell the volumes of {name}");
    };
    let (prefix, number, suffix) = (&parts[1], &parts[2], &parts[3]);
    let width = number.len();
    let mut volumes = Vec::new();
    for index in 1..=count {
        let volume = PathBuf::from(format!("{prefix}{index:0width$}{suffix}"));
        if !volume.is_file() {
            bail!("missing volume {}", volume.display());
        }
        volumes.push(volume);
    }
    Ok(volumes)
}
