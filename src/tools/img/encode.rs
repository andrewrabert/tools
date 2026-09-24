//! Single-step conversions, each a thin wrapper over one external encoder.

use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::tools::img::cli::{Kernel, ResizeMode};
use crate::tools::img::mozjpeg;
use crate::tools::img::probe;
use crate::tools::img::process;

/// Encoder settings chosen on the `convert` command line.
pub struct Settings {
    pub jpeg_quality: u8,
    pub jxl_distance: &'static str,
    pub jxl_effort: &'static str,
}

impl Settings {
    pub const LOSSLESS_DISTANCE: &str = "0.0";
    /// Visually lossless.
    pub const LOSSY_DISTANCE: &str = "1.0";
    pub const FULL_EFFORT: &str = "10";
    pub const FAST_EFFORT: &str = "7";
}

pub fn to_jpeg(source: &Path, dest: &Path, settings: &Settings) -> Result<()> {
    let mut command;
    if process::which("cjpegli").is_some() {
        command = Command::new("cjpegli");
        command
            .arg(format!("--quality={}", settings.jpeg_quality))
            .arg("--")
            .arg(source)
            .arg(dest);
    } else {
        command = Command::new("cjpeg");
        command
            .arg("-quality")
            .arg(settings.jpeg_quality.to_string())
            .arg("-outfile")
            .arg(dest)
            .arg(source);
    }
    command.stderr(Stdio::null());
    process::run(&mut command)?;

    let data = fs::read(dest).with_context(|| format!("reading {}", dest.display()))?;
    let data = mozjpeg::optimize(&data)?;
    fs::write(dest, data).with_context(|| format!("writing {}", dest.display()))
}

pub fn djxl(source: &Path, dest: &Path) -> Result<()> {
    let mut command = Command::new("djxl");
    command
        .arg("--")
        .arg(source)
        .arg(dest)
        .stderr(Stdio::null());
    process::run(&mut command)
}

pub fn to_ppm(source: &Path, dest: &Path) -> Result<()> {
    let mut command = Command::new("vips");
    command
        .args(["ppmsave", "--keep=none", "--"])
        .arg(source)
        .arg(dest);
    process::run(&mut command)
}

pub fn to_bmp(source: &Path, dest: &Path) -> Result<()> {
    let mut command = Command::new("vips");
    command
        .args(["magicksave", "--format=BMP", "--"])
        .arg(source)
        .arg(dest);
    process::run(&mut command)
}

pub fn to_png(source: &Path, dest: &Path) -> Result<()> {
    let mut command = Command::new("vips");
    command
        .args(["pngsave", "--compression=0", "--"])
        .arg(source)
        .arg(dest);
    process::run(&mut command)
}

pub fn to_jxl(source: &Path, dest: &Path, settings: &Settings) -> Result<()> {
    let lossless_jpeg = if settings.jxl_distance == Settings::LOSSLESS_DISTANCE {
        1
    } else {
        0
    };
    let mut command = Command::new("cjxl");
    command
        .arg("--quiet")
        .arg(format!("--distance={}", settings.jxl_distance))
        .arg(format!("--effort={}", settings.jxl_effort))
        .arg(format!("--lossless_jpeg={lossless_jpeg}"))
        .arg("--")
        .arg(source)
        .arg(dest);
    process::run(&mut command)
}

pub fn to_tiff_vips(source: &Path, dest: &Path) -> Result<()> {
    let mut command = Command::new("vips");
    command.arg("tiffsave").arg(source).arg(dest);
    process::run(&mut command)
}

pub fn to_jxl_vips(source: &Path, dest: &Path) -> Result<()> {
    let mut command = Command::new("vips");
    command
        .arg("jxlsave")
        .arg(source)
        .arg(dest)
        .arg("--lossless=true");
    process::run(&mut command)
}

/// Exports a Krita document; sandboxed in a transient unit where systemd is around.
pub fn from_kra(source: &Path, dest: &Path) -> Result<()> {
    let mut command;
    if process::which("systemd-run").is_some() {
        command = Command::new("systemd-run");
        command.args(["--user", "--pty", "--same-dir", "--collect", "--", "krita"]);
    } else {
        command = Command::new("krita");
    }
    command
        .args(["--platform", "offscreen", "--export", "--export-filename"])
        .arg(dest)
        .arg("--")
        .arg(source)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    process::run(&mut command)
}

pub fn extract_tiff_page(source: &Path, page: u32, dest: &Path) -> Result<()> {
    let mut paged = OsString::from(source);
    paged.push(format!("[page={page}]"));
    let mut command = Command::new("vips");
    command.arg("copy").arg(paged).arg(dest);
    process::run(&mut command)
}

/// Joins inputs into one multi-page file; magick expands multi-page inputs into their pages.
pub fn combine(sources: &[impl AsRef<Path>], dest: &Path) -> Result<()> {
    let mut command = Command::new("magick");
    for source in sources {
        command.arg(source.as_ref());
    }
    command.arg(dest).stderr(Stdio::piped());
    let output = command
        .spawn()
        .and_then(|child| child.wait_with_output())
        .context("running magick")?;
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    process::check(
        &command,
        output.status,
        (!stderr.is_empty()).then_some(stderr),
    )
}

/// Resizes with `vips resize` when a kernel is chosen, otherwise `vips thumbnail`.
pub fn resize(
    source: &Path,
    dest: &Path,
    width: u32,
    height: Option<u32>,
    mode: ResizeMode,
    kernel: Option<Kernel>,
    scale: Option<f64>,
) -> Result<()> {
    let mut command = Command::new("vips");
    match kernel {
        Some(kernel) => {
            let scale = match scale {
                Some(scale) => scale,
                None => {
                    let (image_width, _) = probe::dimensions(source)?;
                    f64::from(width) / f64::from(image_width)
                }
            };
            command
                .arg("resize")
                .arg(source)
                .arg(dest)
                .arg(scale.to_string())
                .arg(format!("--kernel={}", kernel.vips_name()));
        }
        None => {
            command
                .arg("thumbnail")
                .arg(source)
                .arg(dest)
                .arg(width.to_string());
            if let Some(height) = height {
                command.arg(format!("--height={height}"));
            }
            command.arg(format!("--size={}", mode.vips_size()));
        }
    }
    process::run(&mut command)
}
