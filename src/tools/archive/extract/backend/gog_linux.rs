use std::fs::{self, File, Permissions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::bytes::Regex;
use zip::ZipArchive;
use zip::read::{ArchiveOffset, Config};

use crate::tools::archive::extract::backend::Request;
use crate::tools::archive::extract::listing::Contents;

const GAME_PREFIX: &str = "data/noarch/";
const PERMISSION_BITS: u32 = 0o7777;
const SCRIPT_HEAD_LENGTH: u64 = 10240;

static SCRIPT_LINE_COUNT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"offset=`head -n (\d+) "\$0""#).expect("valid regex"));
static SETUP_ARCHIVE_SIZE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"filesizes="(\d+)""#).expect("valid regex"));

fn captured_number(pattern: &Regex, text: &[u8]) -> Option<u64> {
    let digits = pattern.captures(text)?.get(1)?.as_bytes();
    std::str::from_utf8(digits).ok()?.parse().ok()
}

// The installer is a shell script, then the MojoSetup archive whose size the script records,
// then the zip of game data.
fn game_data_offset(installer: &mut File) -> Result<u64> {
    let mut head = Vec::new();
    installer
        .by_ref()
        .take(SCRIPT_HEAD_LENGTH)
        .read_to_end(&mut head)?;
    let line_count = captured_number(&SCRIPT_LINE_COUNT, &head)
        .context("the script does not say how long it is")?;

    installer.seek(SeekFrom::Start(0))?;
    let mut reader = BufReader::new(installer);
    let mut script = Vec::new();
    for _ in 0..line_count {
        reader.read_until(b'\n', &mut script)?;
    }
    let setup_archive_size = captured_number(&SETUP_ARCHIVE_SIZE, &script)
        .context("the script does not give the size of the MojoSetup archive")?;
    Ok(script.len() as u64 + setup_archive_size)
}

fn open(installer: &Path) -> Result<ZipArchive<File>> {
    let mut file =
        File::open(installer).with_context(|| format!("opening {}", installer.display()))?;
    let offset = game_data_offset(&mut file)
        .with_context(|| format!("locating the game data in {}", installer.display()))?;
    let config = Config {
        archive_offset: ArchiveOffset::Known(offset),
    };
    ZipArchive::with_config(config, file)
        .with_context(|| format!("reading {}", installer.display()))
}

pub fn contents(installer: &Path) -> Result<Contents> {
    let archive = open(installer)?;
    Ok(Contents::named(
        archive
            .file_names()
            .filter_map(|name| name.strip_prefix(GAME_PREFIX))
            .filter(|name| !name.is_empty())
            .map(|name| name.as_bytes().to_vec()),
    ))
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    request.keeping_existing()?;
    let directory = request.directory()?;
    let mut archive = open(request.archive)?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("reading {}", request.archive.display()))?;
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let Ok(local) = enclosed.strip_prefix(GAME_PREFIX) else {
            continue;
        };
        if local.as_os_str().is_empty() {
            continue;
        }
        let target = directory.join(local);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("creating {}", target.display()))?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            let mut output =
                File::create(&target).with_context(|| format!("creating {}", target.display()))?;
            io::copy(&mut entry, &mut output)
                .with_context(|| format!("writing {}", target.display()))?;
        }
        if let Some(mode) = entry.unix_mode().filter(|mode| *mode != 0) {
            fs::set_permissions(&target, Permissions::from_mode(mode & PERMISSION_BITS))
                .with_context(|| format!("setting the mode of {}", target.display()))?;
        }
    }
    Ok(())
}
