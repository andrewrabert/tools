//! Finds PNGs that are gray in every pixel yet stored as color, and recodes them.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, Result};

use crate::tools::archive::pool;
use crate::tools::img::cli::Fixcolorspace;
use crate::tools::img::mime::{self, Mime};
use crate::tools::img::process;
use crate::tools::img::temp;
use crate::tools::img::{jobs, walk};

const TEMP_PREFIX: &str = ".imgfixcolorspace";

pub fn run(args: Fixcolorspace) -> Result<ExitCode> {
    let jobs = jobs(0)?;
    let mut pngs = Vec::new();
    for path in walk::all_files(&args.paths)? {
        let detected =
            mime::detect(&path).with_context(|| format!("reading {}", path.display()))?;
        if detected.is_some_and(|d| d.mime == Mime::Png) {
            pngs.push(path);
        }
    }

    println!("Checking for incorrect colorspaces");
    let total = pngs.len();
    let mut done = 0;
    let mut wrong: Vec<PathBuf> = Vec::new();
    let mut failure = None;
    pool::run(
        pngs,
        jobs,
        |path| has_incorrect_colorspace(path),
        |path, result| {
            match result {
                Ok(true) => wrong.push(path),
                Ok(false) => {}
                Err(error) => {
                    failure.get_or_insert(error.context(format!("checking {}", path.display())));
                }
            }
            done += 1;
            println!("{done} / {total}");
        },
    );
    if let Some(error) = failure {
        return Err(error);
    }

    println!("Fixing incorrect colorspaces");
    let total = wrong.len();
    let mut done = 0;
    let mut failure = None;
    pool::run(
        wrong,
        jobs,
        |path| fix_colorspace(path),
        |path, result| {
            if let Err(error) = result {
                failure.get_or_insert(error.context(format!("fixing {}", path.display())));
            }
            done += 1;
            println!("{done} / {total}");
        },
    );
    match failure {
        Some(error) => Err(error),
        None => Ok(ExitCode::SUCCESS),
    }
}

fn has_incorrect_colorspace(path: &Path) -> Result<bool> {
    let mut identify = Command::new("magick");
    identify
        .args(["identify", "-quiet", "-format", "%[colorspace]"])
        .arg(path);
    if process::output_text(&mut identify)? == "Gray" {
        return Ok(false);
    }

    // Zero saturation everywhere means the image is gray despite its colorspace.
    let mut saturation = Command::new("magick");
    saturation.arg(path).args([
        "-colorspace",
        "HSB",
        "-channel",
        "green",
        "-separate",
        "+channel",
        "-format",
        "%[fx:100*mean>0?1:0]",
        "info:",
    ]);
    Ok(process::output_text(&mut saturation)? == "0")
}

fn fix_colorspace(path: &Path) -> Result<()> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = temp::with_suffix(dir, TEMP_PREFIX, ".png")?;
    let mut command = Command::new("magick");
    command
        .arg(path)
        .args([
            "-depth",
            "4",
            "-colorspace",
            "Gray",
            "-define",
            "png:compression-level=0",
        ])
        .arg(&*tmp)
        .stderr(Stdio::inherit());
    process::run(&mut command)?;
    temp::replace(tmp, path)
}
