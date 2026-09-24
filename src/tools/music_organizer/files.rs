use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use chardetng::EncodingDetector;
use nix::unistd::{Gid, Uid, chown};
use tempfile::Builder;

use crate::tools::music_organizer::log::Log;
use crate::tools::music_organizer::name::safe_rename;
use crate::tools::music_organizer::tags::{AUDIO_EXTS, FLAC_EXT};

const IMAGE_EXTS: [&str; 2] = ["jpg", "png"];
const IMAGE_STEM: &str = "cover";
const CUE_EXT: &str = "cue";
const SPLIT_DIR_PREFIX: &str = ".shntool_split";
const DIR_MODE: u32 = 0o755;
const FILE_MODE: u32 = 0o644;

// Preferred suffix first.
const MIME_SUFFIXES: [(&str, &[&str]); 8] = [
    ("application/pdf", &["pdf"]),
    ("audio/flac", &["flac"]),
    ("audio/mpeg", &["mp3"]),
    ("audio/ogg", &["ogg", "opus"]),
    ("image/gif", &["gif"]),
    ("image/jpeg", &["jpg"]),
    ("image/png", &["png"]),
    ("text/plain", &["txt", "accurip", "cue", "log", "nfo"]),
];

pub fn set_permissions(path: &Path) -> Result<()> {
    chown(path, Some(Uid::current()), Some(Gid::current()))
        .with_context(|| format!("changing the owner of {}", path.display()))?;
    let mode = if path.is_dir() { DIR_MODE } else { FILE_MODE };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("changing the mode of {}", path.display()))
}

pub fn files_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry
            .with_context(|| format!("reading {}", dir.display()))?
            .path();
        if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

pub fn extension(path: &Path) -> &str {
    path.extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
}

fn stem(path: &Path) -> &str {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
}

pub fn rename_extensions(log: &mut Log, dir: &Path, dry_run: bool) -> Result<()> {
    for file in files_in(dir)? {
        let mime = mime_type(&file)?;
        let Some((_, expected)) = MIME_SUFFIXES.iter().find(|(known, _)| *known == mime) else {
            log.error(format!(
                "Unhandled MIME: \"{mime}\" for \"{}\"",
                file.display()
            ));
            continue;
        };
        let extension = extension(&file);
        if expected.contains(&extension) {
            continue;
        }
        let lowered = extension.to_lowercase();
        if expected.contains(&lowered.as_str()) {
            safe_rename(log, &file, &format!("{}.{lowered}", stem(&file)), dry_run);
        } else if mime == "text/plain" {
            log.error(format!(
                "Unhandled text/plain suffix: \"{}\"",
                file.display()
            ));
        } else {
            safe_rename(
                log,
                &file,
                &format!("{}.{}", stem(&file), expected[0]),
                dry_run,
            );
        }
    }
    Ok(())
}

fn mime_type(file: &Path) -> Result<String> {
    let output = Command::new("file")
        .args(["--brief", "--mime-type", "--"])
        .arg(file)
        .stderr(Stdio::inherit())
        .output()
        .context("running file")?;
    if !output.status.success() {
        bail!("file failed: {}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn organize_images(log: &mut Log, dir: &Path, dry_run: bool) -> Result<()> {
    let files = files_in(dir)?;
    if !files
        .iter()
        .any(|file| AUDIO_EXTS.contains(&extension(file)))
    {
        return Ok(());
    }
    let images: Vec<&PathBuf> = files
        .iter()
        .filter(|file| IMAGE_EXTS.contains(&extension(file)))
        .collect();
    match images.as_slice() {
        [image] => {
            let target = image.with_file_name(format!("{IMAGE_STEM}.{}", extension(image)));
            if **image != target {
                if dry_run {
                    log.info(format!(
                        "Would rename image: {} -> {}",
                        image.display(),
                        target.display()
                    ));
                } else {
                    fs::rename(image, &target)
                        .with_context(|| format!("renaming {}", image.display()))?;
                }
            }
        }
        [] => log.error(format!("No images: {}", dir.display())),
        _ => log.error(format!("Multiple images: {}", dir.display())),
    }
    Ok(())
}

pub fn check_cue_files(log: &mut Log, dir: &Path, dry_run: bool) -> Result<()> {
    let files = files_in(dir)?;
    let flacs: Vec<&PathBuf> = files
        .iter()
        .filter(|file| extension(file) == FLAC_EXT)
        .collect();
    if flacs.is_empty() {
        return Ok(());
    }
    let cues: Vec<&PathBuf> = files
        .iter()
        .filter(|file| extension(file) == CUE_EXT)
        .collect();
    if cues.len() > 1 {
        log.error(format!("Multiple .cue files: {}", dir.display()));
    } else if let ([cue], [flac]) = (cues.as_slice(), flacs.as_slice()) {
        match cue_track_count(log, cue, dry_run)? {
            0 => log.error(format!("Bad cue file: {}", cue.display())),
            1 => {}
            _ => split_flac(log, flac, cue, dry_run)?,
        }
    }
    Ok(())
}

/// The number of tracks in a cue sheet, converting the sheet to UTF-8 on the way.
fn cue_track_count(log: &mut Log, cue: &Path, dry_run: bool) -> Result<usize> {
    let data = fs::read(cue).with_context(|| format!("reading {}", cue.display()))?;
    let mut detector = EncodingDetector::new();
    detector.feed(&data, true);
    let (text, _, _) = detector.guess(None, true).decode(&data);
    if text.as_bytes() != data {
        if dry_run {
            log.info(format!("Would convert encoding for: {}", cue.display()));
        } else {
            fs::write(cue, text.as_bytes())
                .with_context(|| format!("writing {}", cue.display()))?;
        }
    }
    Ok(text
        .lines()
        .filter(|line| line.trim().starts_with("TRACK "))
        .count())
}

fn split_flac(log: &mut Log, flac: &Path, cue: &Path, dry_run: bool) -> Result<()> {
    if dry_run {
        log.info(format!("Would split {}", flac.display()));
        return Ok(());
    }
    log.info(format!("Splitting {}", flac.display()));
    let parent = cue
        .parent()
        .with_context(|| format!("{} has no parent", cue.display()))?;
    let split_dir = Builder::new()
        .prefix(SPLIT_DIR_PREFIX)
        .tempdir_in(parent)
        .with_context(|| format!("creating a temporary directory in {}", parent.display()))?;
    let cue_name = cue
        .file_name()
        .with_context(|| format!("{} has no name", cue.display()))?;
    let flac_name = flac
        .file_name()
        .with_context(|| format!("{} has no name", flac.display()))?;
    let status = Command::new("shntool")
        .args(["split", "-q", "-d"])
        .arg(split_dir.path())
        .arg("-f")
        .arg(cue_name)
        .args([
            "-o",
            "flac flac --best --verify --output-name split%f -",
            "-t",
            "%n",
        ])
        .arg(flac_name)
        .current_dir(parent)
        .status()
        .context("running shntool")?;
    if !status.success() {
        bail!("shntool failed: {status}");
    }
    for track in files_in(split_dir.path())? {
        let name = track
            .file_name()
            .with_context(|| format!("{} has no name", track.display()))?;
        fs::rename(&track, parent.join(name))
            .with_context(|| format!("moving {}", track.display()))?;
    }
    fs::remove_file(flac).with_context(|| format!("removing {}", flac.display()))
}
