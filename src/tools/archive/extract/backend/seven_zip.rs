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
            .args(["-ba", "l", "-slt"])
            .arg(password_flag(None))
            .arg("--")
            .arg(archive)
            .stdin(Stdio::null()),
    )?;
    parse_listing(&output)
}

fn parse_listing(output: &[u8]) -> Result<Contents> {
    let mut contents = Contents::default();
    let mut block = Block::default();
    let mut lines = listing::lines(output);
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    // `7z -ba l -slt` output from 7-Zip 26.04. Fixtures that 7z cannot produce on Linux are
    // hand-edited copies of a real one; each test using one says what was edited.
    macro_rules! fixture {
        ($name:literal) => {
            include_bytes!(concat!("testdata/seven_zip/", $name, ".bin"))
        };
    }

    fn entries(output: &[u8]) -> BTreeMap<Vec<u8>, Entry> {
        parse_listing(output)
            .expect("valid listing")
            .into_iter()
            .collect()
    }

    fn file_entry(output: &[u8]) -> Entry {
        entries(output)
            .remove(b"file".as_slice())
            .expect("file is listed")
    }

    // Each fixture lists `dir` and `dir/file`.
    fn assert_directory(output: &[u8]) {
        let entries = entries(output);
        assert!(entries.contains_key(b"dir/file".as_slice()));
        assert!(!entries.contains_key(b"dir".as_slice()));
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "{actual} is not {expected}"
        );
    }

    #[test]
    fn read_only_directory() {
        assert_directory(fixture!("read_only_directory"));
    }

    // plain_directory with Attributes edited to HD.
    #[test]
    fn hidden_directory() {
        assert_directory(fixture!("hidden_directory"));
    }

    // plain_directory with Attributes edited to DA.
    #[test]
    fn archive_bit_directory() {
        assert_directory(fixture!("archive_bit_directory"));
    }

    #[test]
    fn plain_directory() {
        assert_directory(fixture!("plain_directory"));
    }

    // plain_directory with Attributes edited to D, as for an archive made on Windows.
    #[test]
    fn windows_directory() {
        assert_directory(fixture!("windows_directory"));
    }

    // The directory's Attributes are edited to be empty, leaving only Folder to mark it.
    #[test]
    fn zip_folder_is_a_directory() {
        assert_directory(fixture!("zip_folder_is_a_directory"));
    }

    #[test]
    fn zip_file_without_windows_attributes() {
        let entries = entries(fixture!("zip_file_without_windows_attributes"));
        assert!(entries.contains_key(b"dir/file".as_slice()));
    }

    #[test]
    fn file_is_listed() {
        let entries = entries(fixture!("file"));
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key(b"file".as_slice()));
    }

    #[test]
    fn path_containing_separator() {
        let entries = entries(fixture!("path_containing_separator"));
        assert!(entries.contains_key(b"dir/a = b".as_slice()));
    }

    #[test]
    fn size() {
        assert_eq!(file_entry(fixture!("file")).size, Some(42));
    }

    // file with Size edited to be empty, as for .dmg entries.
    #[test]
    fn empty_size() {
        assert_eq!(file_entry(fixture!("empty_size")).size, None);
    }

    // file with Modified edited to be empty.
    #[test]
    fn empty_modified() {
        assert_eq!(file_entry(fixture!("empty_modified")).mtime, None);
    }

    #[test]
    fn modified() {
        let expected = mtime("2026-10-25 01:46:08").expect("valid time") + 0.1137229;
        let actual = file_entry(fixture!("file")).mtime.expect("mtime");
        assert_close(actual, expected);
    }

    // The fraction's leading zeros are dropped on purpose; see `mtime`.
    #[test]
    fn modified_fraction_drops_leading_zeros() {
        let seconds = mtime("2026-10-25 01:46:08").expect("valid time");
        assert_close(
            mtime("2026-10-25 01:46:08.05").expect("valid time"),
            seconds + 0.5,
        );
    }

    // file without its final blank line.
    #[test]
    fn last_record_without_blank_line() {
        let entries = entries(fixture!("last_record_without_blank_line"));
        assert!(entries.contains_key(b"file".as_slice()));
    }

    // file with CRLF line endings.
    #[test]
    fn crlf_line_endings() {
        let entries = entries(fixture!("crlf_line_endings"));
        assert!(entries.contains_key(b"file".as_slice()));
    }

    #[test]
    fn empty_output() {
        assert!(parse_listing(b"").expect("valid listing").is_empty());
    }

    // file without its Path line.
    #[test]
    fn record_without_path_is_skipped() {
        assert!(entries(fixture!("record_without_path")).is_empty());
    }

    // file with its Encrypted line replaced by `garbage`.
    #[test]
    fn line_without_separator_is_an_error() {
        assert!(parse_listing(fixture!("line_without_separator")).is_err());
    }

    // file with Size edited to `x`.
    #[test]
    fn unparseable_size_is_an_error() {
        assert!(parse_listing(fixture!("unparseable_size")).is_err());
    }

    // file with Modified edited to `soon`.
    #[test]
    fn unparseable_modified_is_an_error() {
        assert!(parse_listing(fixture!("unparseable_modified")).is_err());
    }
}
