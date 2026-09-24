//! Hashes of decoded pixels, so a re-encoded JPEG or JPEG XL can be checked
//! against its original.

use std::io;
use std::path::Path;
use std::process::{ChildStdout, Command, ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::tools::img::cli::Compare;
use crate::tools::img::mime::{self, Mime};
use crate::tools::img::process;

pub fn run(args: Compare) -> Result<ExitCode> {
    for path in &args.paths {
        let detected = mime::detect(path).with_context(|| format!("reading {}", path.display()))?;
        let hash = match detected.map(|d| d.mime) {
            Some(Mime::Jxl) => sha256_of_jxl(path)?,
            Some(Mime::Jpeg) => sha256_of_decoded(Input::Path(path))?,
            Some(other) => bail!("compare is not implemented for {}", other.as_str()),
            None => bail!("unrecognized image: {}", path.display()),
        };
        println!("{hash}");
    }
    Ok(ExitCode::SUCCESS)
}

enum Input<'a> {
    Path(&'a Path),
    Stream(ChildStdout),
}

fn sha256_of_jxl(path: &Path) -> Result<String> {
    let mut djxl = Command::new("djxl");
    djxl.args(["--quiet", "--output_format=ppm", "--"])
        .arg(path)
        .arg("-")
        .stdout(Stdio::piped());
    let mut decoding = djxl.spawn().context("running djxl")?;
    let pixels = decoding.stdout.take().context("djxl has no stdout")?;
    let hash = sha256_of_decoded(Input::Stream(pixels))?;
    let status = decoding.wait().context("running djxl")?;
    process::check(&djxl, status, None)?;
    Ok(hash)
}

/// Normalizes through `vips copy` and ImageMagick (which drops the comment) before hashing.
fn sha256_of_decoded(input: Input<'_>) -> Result<String> {
    let mut copy = Command::new("vips");
    copy.arg("copy");
    match input {
        Input::Path(path) => {
            copy.arg(path);
        }
        Input::Stream(stream) => {
            copy.arg("stdin[]").stdin(Stdio::from(stream));
        }
    }
    copy.arg(".ppm[]").stdout(Stdio::piped());
    let mut copying = copy.spawn().context("running vips")?;
    let ppm = copying.stdout.take().context("vips has no stdout")?;

    let mut magick = Command::new("magick");
    magick
        .args(["ppm:-", "-set", "comment", "", "ppm:-"])
        .stdin(Stdio::from(ppm))
        .stdout(Stdio::piped());
    let mut converting = magick.spawn().context("running magick")?;
    let mut normalized = converting.stdout.take().context("magick has no stdout")?;

    let mut hasher = Sha256::new();
    io::copy(&mut normalized, &mut hasher).context("reading the output of magick")?;

    let status = copying.wait().context("running vips")?;
    process::check(&copy, status, None)?;
    let status = converting.wait().context("running magick")?;
    process::check(&magick, status, None)?;
    Ok(format!("{:x}", hasher.finalize()))
}
