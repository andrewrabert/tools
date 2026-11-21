use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::tools::img::process;

fn vipsheader(path: &Path, fields: &[&str]) -> Result<Vec<String>> {
    let mut command = Command::new("vipsheader");
    for field in fields {
        command.arg("-f").arg(field);
    }
    command.arg("--").arg(path);
    let text = process::output_text(&mut command)?;
    let values: Vec<String> = text.lines().map(str::to_owned).collect();
    if values.len() != fields.len() {
        bail!(
            "vipsheader reported {} fields for {}, expected {}",
            values.len(),
            path.display(),
            fields.len()
        );
    }
    Ok(values)
}

fn number(text: &str, what: &str, path: &Path) -> Result<u32> {
    text.trim()
        .parse()
        .with_context(|| format!("parsing {what} {text:?} of {}", path.display()))
}

pub fn dimensions(path: &Path) -> Result<(u32, u32)> {
    let values = vipsheader(path, &["width", "height"])?;
    Ok((
        number(&values[0], "width", path)?,
        number(&values[1], "height", path)?,
    ))
}

pub fn is_animated_gif(path: &Path) -> Result<bool> {
    let values = vipsheader(path, &["vips-loader", "n-pages"])?;
    if values[0].trim() != "gifload" {
        bail!("not a gif file: {}", path.display());
    }
    Ok(number(&values[1], "page count", path)? > 1)
}

pub fn tiff_pages(path: &Path) -> Result<u32> {
    let values = vipsheader(path, &["n-pages"])?;
    number(&values[0], "page count", path)
}

/// The frame count ImageMagick reports; a dynamic-wallpaper HEIC repeats it once per line.
pub fn heif_frames(path: &Path) -> Result<u32> {
    let mut command = Command::new("magick");
    command
        .arg("convert")
        .arg(path)
        .args(["-format", "%n\n", "info:"])
        .stderr(Stdio::null());
    let text = process::output_text(&mut command)?;
    let counts = text
        .lines()
        .map(|line| number(line, "frame count", path))
        .collect::<Result<BTreeSet<u32>>>()?;
    if counts.len() != 1 {
        bail!(
            "magick reported inconsistent frame counts for {}",
            path.display()
        );
    }
    Ok(counts.into_iter().next().unwrap_or(0))
}
