//! SVG rasterization: a headless Chromium when one is installed, else Inkscape.

use std::fs;
use std::path::{self, Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::tools::img::probe;
use crate::tools::img::process;
use crate::tools::img::temp;

const TEMP_PREFIX: &str = ".imgconvert_svg_";
const BORDER: u32 = 1;

pub fn to_png(
    source: &Path,
    dest: &Path,
    max_w: Option<u32>,
    max_h: Option<u32>,
    scale: Option<f64>,
) -> Result<()> {
    if let Some(chromium) = browser() {
        return chromium_to_png(&chromium, source, dest, max_w, max_h, scale);
    }
    let (max_w, max_h) = match scale {
        Some(scale) => {
            let (width, height) = probe::dimensions(source)?;
            (Some(scaled(width, scale)), Some(scaled(height, scale)))
        }
        None => (max_w, max_h),
    };
    inkscape_to_png(source, dest, max_w, max_h)
}

fn browser() -> Option<PathBuf> {
    process::which("chromium").or_else(|| process::which("google-chrome-stable"))
}

fn scaled(size: u32, scale: f64) -> u32 {
    (f64::from(size) * scale).round() as u32
}

fn inkscape_to_png(
    source: &Path,
    dest: &Path,
    max_w: Option<u32>,
    max_h: Option<u32>,
) -> Result<()> {
    let mut command = Command::new("inkscape");
    command.args(["--export-type=png", "--export-area-page"]);
    if let Some(width) = max_w {
        command.arg(format!("--export-width={width}"));
    }
    if let Some(height) = max_h {
        command.arg(format!("--export-height={height}"));
    }
    command
        .arg("-o")
        .arg(dest)
        .arg("--")
        .arg(source)
        .stderr(Stdio::null());
    process::run(&mut command)
}

fn file_url(path: &Path) -> Result<String> {
    let absolute = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
    Ok(format!("file://{}", absolute.display()))
}

fn parent(dest: &Path) -> &Path {
    dest.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn chromium_to_png(
    chromium: &Path,
    source: &Path,
    dest: &Path,
    max_w: Option<u32>,
    max_h: Option<u32>,
    scale: Option<f64>,
) -> Result<()> {
    let source_url = file_url(source)?;
    let dir = parent(dest);
    let (natural_w, natural_h) = natural_size(chromium, &source_url, dir)?;

    let (target_w, target_h) = match (scale, max_w, max_h) {
        (Some(scale), _, _) => (scaled(natural_w, scale), scaled(natural_h, scale)),
        (None, Some(w), Some(h)) => (w, h),
        (None, Some(w), None) => (w, ratio(natural_h, w, natural_w)),
        (None, None, Some(h)) => (ratio(natural_w, h, natural_h), h),
        (None, None, None) => (natural_w, natural_h),
    };

    // The SVG as an <img> with a magenta border marks the content bounds; the
    // window is twice the size so overflow past the viewBox still lands on screen.
    let render_html = format!(
        "<!DOCTYPE html><html><head>\
         <style>*{{margin:0;padding:0}}html,body{{overflow:hidden}}\
         img{{display:block;border:{BORDER}px solid #ff00ff}}</style></head>\
         <body><img src=\"{source_url}\" width=\"{target_w}\" height=\"{target_h}\">\
         </body></html>"
    );
    let render_page = temp::with_suffix(dir, TEMP_PREFIX, ".html")?;
    fs::write(&render_page, render_html)
        .with_context(|| format!("writing {}", render_page.display()))?;
    let screenshot = temp::with_suffix(dir, TEMP_PREFIX, ".png")?;

    let mut command = Command::new(chromium);
    command
        .args(["--headless=new", "--disable-gpu"])
        .arg(format!("--screenshot={}", screenshot.display()))
        .arg(format!("--window-size={},{}", target_w * 2, target_h * 2))
        .arg("--default-background-color=00000000")
        .arg(file_url(&render_page)?)
        .stderr(Stdio::null());
    process::run(&mut command)?;

    let mut identify = Command::new("magick");
    identify
        .args(["identify", "-format", "%@"])
        .arg(&*screenshot);
    let geometry = process::output_text(&mut identify)?;
    let (box_w, box_h, box_x, box_y) = parse_geometry(geometry.trim())?;

    let mut crop = Command::new("vips");
    crop.arg("crop")
        .arg(&*screenshot)
        .arg(dest)
        .arg((box_x + BORDER).to_string())
        .arg((box_y + BORDER).to_string())
        .arg((box_w - 2 * BORDER).to_string())
        .arg((box_h - 2 * BORDER).to_string());
    process::run(&mut crop)
}

fn ratio(size: u32, numerator: u32, denominator: u32) -> u32 {
    (f64::from(size) * f64::from(numerator) / f64::from(denominator)).round() as u32
}

/// Loads the SVG as an <img> and reads its natural size out of the document title.
fn natural_size(chromium: &Path, source_url: &str, dir: &Path) -> Result<(u32, u32)> {
    let probe_html = format!(
        "<!DOCTYPE html><html><head><title>pending</title></head><body>\
         <img src=\"{source_url}\" onload=\"document.title=this.naturalWidth+','+this.naturalHeight\">\
         </body></html>"
    );
    let probe_page = temp::with_suffix(dir, TEMP_PREFIX, ".html")?;
    fs::write(&probe_page, probe_html)
        .with_context(|| format!("writing {}", probe_page.display()))?;
    let mut command = Command::new(chromium);
    command
        .args([
            "--headless=new",
            "--disable-gpu",
            "--dump-dom",
            "--virtual-time-budget=5000",
        ])
        .arg(file_url(&probe_page)?)
        .stderr(Stdio::null());
    let dom = process::output_text(&mut command)?;

    let title = dom
        .split_once("<title>")
        .and_then(|(_, rest)| rest.split_once("</title>"))
        .map(|(title, _)| title)
        .unwrap_or_default();
    let size = title
        .split_once(',')
        .and_then(|(w, h)| Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?)));
    match size {
        Some(size) => Ok(size),
        None => bail!("failed to probe SVG dimensions via chromium"),
    }
}

/// Parses ImageMagick's `WxH+X+Y` trim geometry.
fn parse_geometry(geometry: &str) -> Result<(u32, u32, u32, u32)> {
    let parsed = (|| {
        let (size, offset) = geometry.split_once('+')?;
        let (w, h) = size.split_once('x')?;
        let (x, y) = offset.split_once('+')?;
        Some((
            w.parse().ok()?,
            h.parse().ok()?,
            x.parse().ok()?,
            y.parse().ok()?,
        ))
    })();
    parsed.with_context(|| format!("unexpected geometry from magick: {geometry:?}"))
}
