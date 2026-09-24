//! In-place lossless optimizers, one per format.

use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::tools::img::exif;
use crate::tools::img::mime::Mime;
use crate::tools::img::mozjpeg;
use crate::tools::img::process;
use crate::tools::img::temp;

const TEMP_PREFIX: &str = ".imgoptim_";
const UPRIGHT: u8 = 1;

#[derive(Clone, Copy)]
pub struct Options {
    pub fast: bool,
    pub strip: bool,
    pub bigtiff: bool,
}

pub struct Optimized {
    pub before: u64,
    pub after: u64,
    pub sha256: Option<String>,
}

type Optimizer = fn(&Path, &Path, Options) -> Result<()>;

fn optimizer(mime: Mime) -> Option<Optimizer> {
    let optimizer: Optimizer = match mime {
        Mime::Gif => gif,
        Mime::Jpeg => jpeg,
        Mime::Jxl => jxl,
        Mime::Png => png,
        Mime::Svg => svg,
        Mime::Tiff => tiff,
        _ => return None,
    };
    Some(optimizer)
}

pub fn supports(mime: Mime) -> bool {
    optimizer(mime).is_some()
}

/// Optimizes into a sibling temporary file and keeps it only when smaller.
pub fn in_place(
    path: &Path,
    mime: Mime,
    options: Options,
    compute_hash: bool,
) -> Result<Optimized> {
    let optimize =
        optimizer(mime).with_context(|| format!("no optimizer for {}", mime.as_str()))?;
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let before = size_of(path)?;

    let tmp = temp::sibling(dir, TEMP_PREFIX, path)?;
    optimize(path, &tmp, options)?;
    let after = size_of(&tmp)?;
    let after = if after < before {
        temp::replace(tmp, path)?;
        after
    } else {
        before
    };

    let sha256 = if compute_hash {
        Some(sha256_of(path)?)
    } else {
        None
    };
    Ok(Optimized {
        before,
        after,
        sha256,
    })
}

fn size_of(path: &Path) -> Result<u64> {
    Ok(fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))?
        .len())
}

pub fn sha256_of(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    File::open(path)
        .and_then(|mut file| io::copy(&mut file, &mut hasher))
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn jpeg(source: &Path, dest: &Path, options: Options) -> Result<()> {
    let mut command = Command::new("jpegoptim");
    command
        .args(["--quiet", "--all-progressive", "--force", "--stdout"])
        .arg(if options.strip {
            "--strip-all"
        } else {
            "--strip-none"
        })
        .arg("--")
        .arg(source);
    let data = process::output(&mut command)?;
    let data = mozjpeg::optimize(&data)?;
    fs::write(dest, data).with_context(|| format!("writing {}", dest.display()))?;

    if options.strip {
        // Lossless rotation is not always possible, so keep the EXIF orientation instead.
        if let Some(orientation) = exif::orientation(source)?
            && orientation != UPRIGHT
        {
            exif::set_orientation(dest, orientation)?;
        }
    } else {
        exif::copy_metadata(source, dest)?;
    }
    Ok(())
}

fn jxl(source: &Path, dest: &Path, _options: Options) -> Result<()> {
    let mut command = Command::new("cjxl");
    command
        .args([
            "--quiet",
            "--distance=0.0",
            "--effort=10",
            "--lossless_jpeg=1",
            "--",
        ])
        .arg(source)
        .arg(dest)
        .stderr(Stdio::null());
    process::run(&mut command)
}

fn png(source: &Path, dest: &Path, options: Options) -> Result<()> {
    let mut command = Command::new("oxipng");
    command.args(["--quiet", "--out"]).arg(dest);
    if options.fast {
        command.arg("--fast");
    } else {
        command.args(["--zopfli", "--opt", "max"]);
    }
    if options.strip {
        command.args(["--strip", "safe"]);
    }
    command.arg("--").arg(source);
    process::run(&mut command)
}

fn gif(source: &Path, dest: &Path, options: Options) -> Result<()> {
    let mut command = Command::new("gifsicle");
    command
        .args([
            "--same-loopcount",
            "--same-delay",
            "--no-warnings",
            "--optimize=3",
            "--output",
        ])
        .arg(dest);
    if options.strip {
        command.args(["--no-comments", "--no-names", "--no-extensions"]);
    }
    command.arg("--").arg(source);
    process::run(&mut command)
}

fn svg(source: &Path, dest: &Path, _options: Options) -> Result<()> {
    let mut command = Command::new("svgo");
    command
        .args(["--quiet", "--multipass", "--input"])
        .arg(source)
        .arg("--output")
        .arg(dest);
    process::run(&mut command)
}

fn tiff(source: &Path, dest: &Path, options: Options) -> Result<()> {
    // tiffcp drops some metadata, so only run it when stripping was asked for.
    if !options.strip {
        bail!("TIFF requires --strip");
    }
    let mut command = Command::new("tiffcp");
    command.args(["-c", "zstd"]);
    if options.bigtiff {
        command.arg("-8");
    }
    command.arg("--").arg(source).arg(dest);
    process::run(&mut command)
}
