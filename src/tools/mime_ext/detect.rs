use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Paths per `file` invocation, well under the argument length limit.
const BATCH: usize = 256;

/// The lowercased MIME type `file` reports for each path, in order.
pub fn mime_types(paths: &[PathBuf]) -> Result<Vec<String>> {
    let mut types = Vec::with_capacity(paths.len());
    for batch in paths.chunks(BATCH) {
        types.extend(detect(batch)?);
    }
    Ok(types)
}

fn detect(paths: &[PathBuf]) -> Result<Vec<String>> {
    let output = Command::new("file")
        .args(["--brief", "--mime-type", "--dereference", "--"])
        .args(paths)
        .stderr(Stdio::inherit())
        .output()
        .context("running file")?;
    if !output.status.success() {
        bail!("file failed: {}", output.status);
    }
    let text = String::from_utf8(output.stdout).context("file wrote non-UTF-8 output")?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != paths.len() {
        bail!(
            "file reported {} types for {} paths",
            lines.len(),
            paths.len()
        );
    }
    lines
        .into_iter()
        .zip(paths)
        .map(|(line, path)| mime_type(line, path))
        .collect()
}

/// `file` puts its error message in place of the type of a file it cannot read.
fn mime_type(line: &str, path: &Path) -> Result<String> {
    let line = line.trim();
    let well_formed = line
        .split_once('/')
        .is_some_and(|(kind, subtype)| !kind.is_empty() && !subtype.is_empty())
        && !line.contains(char::is_whitespace);
    if !well_formed {
        bail!("file could not identify {}: {line}", path.display());
    }
    Ok(line.to_ascii_lowercase())
}
