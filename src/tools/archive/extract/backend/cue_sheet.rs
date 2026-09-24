use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::cue::{self, CueSheet, Datatype};
use crate::tools::archive::extract::backend::{Request, TEMP_PREFIX, file_name};
use crate::tools::archive::extract::listing::Contents;
use crate::tools::archive::process;
use crate::tools::archive::temp;

const CHUNK_PREFIX: &str = "chunk";
const MERGED_NAME: &str = "merged";

/// The suffix bchunk gives a track of `datatype`.
fn suffix(datatype: &Datatype) -> Result<&'static str> {
    match datatype {
        Datatype::Audio => Ok(".wav"),
        Datatype::Mode1_2352 | Datatype::Mode2_2352 => Ok(".iso"),
        Datatype::Cdg
        | Datatype::Mode1_2048
        | Datatype::Mode2_2336
        | Datatype::Cdi2336
        | Datatype::Cdi2352
        | Datatype::Other(_) => bail!("unsupported track type {datatype}"),
    }
}

/// The cue sheet, once every file it names exists.
fn sheet(cue: &Path) -> Result<CueSheet> {
    let data = fs::read(cue).with_context(|| format!("reading {}", cue.display()))?;
    let sheet = CueSheet::new(&data).with_context(|| format!("parsing {}", cue.display()))?;
    for file in data_files(cue, &sheet)? {
        if !file.is_file() {
            bail!("{} named in the cue sheet does not exist", file.display());
        }
    }
    Ok(sheet)
}

/// The files the sheet names, relative to the sheet's directory.
fn data_files(cue: &Path, sheet: &CueSheet) -> Result<BTreeSet<PathBuf>> {
    let parent = cue
        .parent()
        .with_context(|| format!("{} has no parent", cue.display()))?;
    Ok(sheet
        .files()
        .iter()
        .map(|file| parent.join(file.filename()))
        .collect())
}

fn content_name(cue: &Path) -> Result<String> {
    match (cue.file_stem(), cue.extension()) {
        (Some(stem), Some(extension)) if extension.eq_ignore_ascii_case(cue::EXTENSION) => stem
            .to_str()
            .map(str::to_owned)
            .with_context(|| format!("{} is not valid UTF-8", cue.display())),
        _ => bail!("{} must end with .{}", cue.display(), cue::EXTENSION),
    }
}

pub fn contents(cue: &Path) -> Result<Contents> {
    let name = content_name(cue)?;
    let sheet = sheet(cue)?;
    let numbered = sheet.tracks().len() > 1;
    let mut names = Vec::new();
    for (index, track) in sheet.tracks().iter().enumerate() {
        let suffix = suffix(track.datatype())?;
        names.push(if numbered {
            format!("{name}{:02}{suffix}", index + 1).into_bytes()
        } else {
            format!("{name}{suffix}").into_bytes()
        });
    }
    Ok(Contents::named(names))
}

pub fn volumes(cue: &Path) -> Result<Vec<PathBuf>> {
    let mut volumes = vec![cue.to_owned()];
    volumes.extend(data_files(cue, &sheet(cue)?)?);
    Ok(volumes)
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    request.keeping_existing()?;
    request.without_password()?;
    let directory = request.directory()?;
    let cue = request.archive;
    let name = content_name(cue)?;
    let data_files = data_files(cue, &sheet(cue)?)?;
    let scratch = temp::dir(directory, file_name(directory)?, TEMP_PREFIX)?;

    let (source_bin, source_cue) = match (data_files.first(), data_files.len()) {
        (Some(data_file), 1) => (data_file.clone(), cue.to_owned()),
        _ => {
            process::run(
                Command::new("binmerge")
                    .arg("--outdir")
                    .arg(scratch.path())
                    .arg(cue)
                    .arg(MERGED_NAME),
            )?;
            (
                PathBuf::from(format!("{MERGED_NAME}.bin")),
                PathBuf::from(format!("{MERGED_NAME}.{}", cue::EXTENSION)),
            )
        }
    };
    process::run(
        Command::new("bchunk")
            .arg("-w")
            .arg(source_bin)
            .arg(source_cue)
            .arg(CHUNK_PREFIX)
            .current_dir(scratch.path())
            .stdout(Stdio::null()),
    )?;

    let mut produced = Vec::new();
    for entry in fs::read_dir(scratch.path())
        .with_context(|| format!("listing {}", scratch.path().display()))?
    {
        let entry = entry.with_context(|| format!("listing {}", scratch.path().display()))?;
        produced.push(entry.path());
    }

    let mut moves = Vec::new();
    for source in &produced {
        let chunk_name = file_name(source)?.to_string_lossy();
        let mut target_name = OsString::from(&name);
        if produced.len() == 1 {
            if let Some(extension) = source.extension() {
                target_name.push(".");
                target_name.push(extension);
            }
        } else if let Some(numbered) = chunk_name.strip_prefix(CHUNK_PREFIX) {
            target_name.push(numbered);
        } else {
            continue;
        }
        let target = directory.join(target_name);
        if target.exists() {
            bail!("{} already exists", target.display());
        }
        moves.push((source, target));
    }
    for (source, target) in moves {
        fs::rename(source, &target).with_context(|| format!("moving to {}", target.display()))?;
    }
    Ok(())
}
