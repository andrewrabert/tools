use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::tools::img::process;
use crate::tools::img::temp;

const TIFF_TAGS: [&str; 6] = [
    "Make",
    "Model",
    "ResolutionUnit",
    "Software",
    "XResolution",
    "YResolution",
];

/// Sub-second tags, each with the plain tag to fall back on.
const TIFF_TAG_MAP: [(&str, &str); 3] = [
    ("SubSecCreateDate", "CreateDate"),
    ("SubSecDateTimeOriginal", "DateTimeOriginal"),
    ("SubSecModifyDate", "ModifyDate"),
];

/// The EXIF orientation, or `None` when the tag is absent or zero.
pub fn orientation(path: &Path) -> Result<Option<u8>> {
    let mut command = Command::new("exiv2");
    command
        .args(["--key", "Exif.Image.Orientation", "--"])
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command.output().context("running exiv2")?;
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if output.status.code() == Some(1) && stderr.is_empty() {
        return Ok(None);
    }
    process::check(
        &command,
        output.status,
        (!stderr.is_empty()).then_some(stderr),
    )?;

    // "Exif.Image.Orientation  Short  1  top, left"
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut rest = stdout.trim_start();
    for _ in 0..3 {
        let end = rest
            .find(char::is_whitespace)
            .with_context(|| format!("unexpected exiv2 output: {stdout:?}"))?;
        rest = rest[end..].trim_start();
    }
    Ok(match rest.trim() {
        "(0)" => None,
        "top, left" => Some(1),
        "top, right" => Some(2),
        "bottom, right" => Some(3),
        "bottom, left" => Some(4),
        "left, top" => Some(5),
        "right, top" => Some(6),
        "right, bottom" => Some(7),
        "left, bottom" => Some(8),
        other => bail!("unknown EXIF orientation {other:?} in {}", path.display()),
    })
}

pub fn set_orientation(path: &Path, orientation: u8) -> Result<()> {
    let mut command = Command::new("exiv2");
    command
        .arg("--Modify")
        .arg(format!("set Exif.Image.Orientation {orientation}"))
        .arg("mo")
        .arg(path);
    process::run_strict(&mut command)
}

/// Copies all metadata with exiv2; warnings count as errors.
pub fn copy_metadata(source: &Path, target: &Path) -> Result<()> {
    let mut extract = Command::new("exiv2");
    extract
        .args(["-ea-", "--"])
        .arg(source)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut extracting = extract.spawn().context("running exiv2")?;
    let metadata = extracting.stdout.take().context("exiv2 has no stdout")?;

    let mut insert = Command::new("exiv2");
    insert
        .args(["-ia-", "--"])
        .arg(target)
        .stdin(Stdio::from(metadata))
        .stderr(Stdio::piped());
    let inserted = insert
        .spawn()
        .and_then(|child| child.wait_with_output())
        .context("running exiv2")?;
    let extracted = extracting.wait_with_output().context("running exiv2")?;
    process::strict(&extract, &extracted)?;
    process::strict(&insert, &inserted)
}

/// Carries the TIFF-relevant tags from `source` into `target` with exiftool.
pub fn copy_tiff_tags(source: &Path, target: &Path) -> Result<()> {
    let mut read = Command::new("exiftool");
    read.args(["-overwrite_original", "-quiet", "-json", "--"])
        .arg(source);
    let json = process::output(&mut read)?;
    let mut entries: Vec<Map<String, Value>> =
        serde_json::from_slice(&json).context("parsing the output of exiftool")?;
    if entries.len() != 1 {
        bail!(
            "exiftool reported {} entries for {}",
            entries.len(),
            source.display()
        );
    }
    let metadata = entries.remove(0);

    let mut kept = Map::new();
    for key in TIFF_TAGS {
        if let Some(value) = metadata.get(key) {
            kept.insert(key.to_owned(), value.clone());
        }
    }
    for (precise, plain) in TIFF_TAG_MAP {
        if let Some(value) = metadata.get(precise).or_else(|| metadata.get(plain)) {
            kept.insert(precise.to_owned(), value.clone());
        }
    }

    let tags = temp::scratch(".img_tags_", ".json")?;
    fs::write(&tags, serde_json::to_vec(&Value::Object(kept))?)
        .with_context(|| format!("writing {}", tags.display()))?;
    let mut json_arg = temp::os("-json=");
    json_arg.push(tags.as_os_str());
    let mut write = Command::new("exiftool");
    write
        .args(["-overwrite_original", "-quiet"])
        .arg(json_arg)
        .arg("--")
        .arg(target);
    process::output(&mut write)?;
    Ok(())
}
