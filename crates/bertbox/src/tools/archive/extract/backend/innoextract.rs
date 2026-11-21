use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::tools::archive::extract::backend::{Destination, Request};
use crate::tools::archive::extract::listing::{self, Contents};
use crate::tools::archive::process;

const MD5_HEX_LENGTH: usize = 32;
const SHA1_HEX_LENGTH: usize = 40;
const GOG_INSTALLER_PREFIX: &str = "setup_";

pub fn contents(archive: &Path) -> Result<Contents> {
    let output = process::capture(
        Command::new("innoextract")
            // --gog is harmless for other installers, and --silent is what upstream recommends
            // for parsable output: https://github.com/dscharrer/innoextract/issues/78
            .args([
                "--gog",
                "--exclude-temp",
                "--silent",
                "--list-checksums",
                "--list-sizes",
                "--",
            ])
            .arg(archive)
            .stderr(Stdio::null()),
    )?;
    let mut names = Vec::new();
    for line in listing::lines(&output) {
        names.push(if line.ends_with(b"/") {
            line.to_vec()
        } else {
            listed_name(line)?.to_vec()
        });
    }
    Ok(Contents::named(names))
}

fn listed_name(line: &[u8]) -> Result<&[u8]> {
    let &[size, hash_type, hash, name] = listing::split_fields(line, 3).as_slice() else {
        return Ok(line);
    };
    let is_size = std::str::from_utf8(size).is_ok_and(|size| size.parse::<u64>().is_ok());
    let hash_length = match hash_type {
        b"MD5" => MD5_HEX_LENGTH,
        b"SHA-1" => SHA1_HEX_LENGTH,
        _ => return Ok(line),
    };
    if !is_size {
        return Ok(line);
    }
    if hash.len() != hash_length {
        bail!(
            "unexpected innoextract checksum: {}",
            String::from_utf8_lossy(line)
        );
    }
    Ok(name)
}

pub fn extract(request: &Request) -> Result<()> {
    request.keeping_existing()?;
    let mut command = Command::new("innoextract");
    command.args(["--silent", "--gog", "--exclude-temp", "--extract"]);

    if !request.members.is_empty() {
        let contents = contents(request.archive)?;
        for member in request.members {
            if !contents.contains(member.as_bytes()) {
                bail!("{member} is not included in the archive");
            }
            command.arg("--include").arg(format!("/{member}"));
        }
    }

    match request.destination {
        Destination::Directory(directory) => process::run(
            command
                .arg("--output-dir")
                .arg(directory)
                .arg(request.archive),
        ),
        Destination::Stream(fd) => {
            let [member] = request.members else {
                bail!("exactly one path must be specified when extracting to stdout");
            };
            let scratch = tempfile::tempdir().context("creating a temporary directory")?;
            process::run(
                command
                    .arg("--output-dir")
                    .arg(scratch.path())
                    .arg(request.archive),
            )?;
            let extracted = scratch.path().join(member);
            let mut source = File::open(&extracted)
                .with_context(|| format!("opening {}", extracted.display()))?;
            let mut output = File::from(fd.try_clone().context("duplicating the output stream")?);
            io::copy(&mut source, &mut output).context("writing to stdout")?;
            Ok(())
        }
    }
}

pub fn volumes(archive: &Path) -> Vec<PathBuf> {
    let mut volumes = vec![archive.to_owned()];
    let is_gog_installer = archive
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(GOG_INSTALLER_PREFIX) && name.ends_with(".exe"));
    if let (true, Some(stem)) = (is_gog_installer, archive.file_stem()) {
        for number in 1u64.. {
            let mut name = stem.to_owned();
            name.push(format!("-{number}.bin"));
            let volume = archive.with_file_name(name);
            if !volume.is_file() {
                break;
            }
            volumes.push(volume);
        }
    }
    volumes
}
