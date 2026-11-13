use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, Result, bail, ensure};
use clap::Args as ClapArgs;

use crate::tools::Tool;

const INSTALL_DIR: &str = "/usr/share/rife-ncnn-vulkan";
const PROGRAM: &str = "rife-ncnn-vulkan";
pub const DEFAULT_MODEL: &str = "rife-v4.25-heavy";
/// rife-ncnn-vulkan fails now and then on what look like random buffer overflows.
const ATTEMPTS: u32 = 5;

#[derive(ClapArgs)]
#[command(
    about = "Interpolate a directory of PNG frames with rife-ncnn-vulkan",
    after_help = models_help()
)]
pub struct Rife {
    #[arg(short, long, value_name = "MODEL", default_value = DEFAULT_MODEL)]
    model: String,
    /// How many times the frame count doubles
    #[arg(short, long, value_name = "NUM", default_value_t = 1)]
    num_passes: u32,
    /// A directory of PNG frames in name order
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    /// The directory to write the frames to, created if missing
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,
}

impl Tool for Rife {
    fn run(self) -> ExitCode {
        match interpolate(&self.input, &self.output, &self.model, self.num_passes) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

/// The installed model names, in order.
fn models() -> Vec<String> {
    let Ok(entries) = fs::read_dir(INSTALL_DIR) else {
        return Vec::new();
    };
    let mut models: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("rife-"))
        .collect();
    models.sort();
    models
}

fn models_help() -> String {
    let models = models();
    if models.is_empty() {
        return format!("No models found in {INSTALL_DIR}");
    }
    let mut help = String::from("Models:\n");
    for model in models {
        help.push_str("  ");
        help.push_str(&model);
        help.push('\n');
    }
    help
}

/// Writes the PNG frames of `input`, with `passes` rounds of interpolated frames between
/// them, to `output` as zero-padded names in order.
pub fn interpolate(input: &Path, output: &Path, model: &str, passes: u32) -> Result<()> {
    ensure!(
        Path::new(INSTALL_DIR).join(model).is_dir(),
        "no model {model:?} in {INSTALL_DIR}"
    );
    let inputs = frames(input)?;
    ensure!(
        inputs.len() >= 2,
        "{} must hold at least two frames",
        input.display()
    );
    fs::create_dir_all(output).with_context(|| format!("creating {}", output.display()))?;

    let final_count = (0..passes).fold(inputs.len(), |count, _| count * 2 - 1);
    let new_total = final_count - inputs.len();
    let mut new_done = 0;
    let mut stderr = io::stderr().lock();

    let mut working: Vec<PathBuf> = inputs.clone();
    for pass in 0..passes {
        let mut next = Vec::with_capacity(working.len() * 2 - 1);
        for (index, pair) in working.windows(2).enumerate() {
            let between = output.join(format!(".rife_pass{pass}_{index}.png"));
            interpolate_pair(&pair[0], &pair[1], &between, model)?;
            next.push(pair[0].clone());
            next.push(between);
            new_done += 1;
            let _ = write!(stderr, "\r({new_done} / {new_total})");
            let _ = stderr.flush();
        }
        next.extend(working.pop());
        working = next;
    }
    if new_total > 0 {
        let _ = writeln!(stderr);
    }

    let width = working.len().to_string().len();
    for (index, path) in working.iter().enumerate() {
        let target = output.join(format!("{index:0width$}.png"));
        if inputs.contains(path) {
            fs::copy(path, &target)
                .with_context(|| format!("copying {} to {}", path.display(), target.display()))?;
        } else {
            fs::rename(path, &target)
                .with_context(|| format!("moving {} to {}", path.display(), target.display()))?;
        }
    }
    Ok(())
}

/// The PNG files of `dir` in name order.
fn frames(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry
            .with_context(|| format!("reading {}", dir.display()))?
            .path();
        ensure!(path.is_file(), "{} is not a file", path.display());
        ensure!(
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png")),
            "{} is not a .png",
            path.display()
        );
        paths.push(path);
    }
    paths.sort();
    Ok(paths)
}

/// Writes the frame halfway between `first` and `second` to `between`.
fn interpolate_pair(first: &Path, second: &Path, between: &Path, model: &str) -> Result<()> {
    let program = Path::new(INSTALL_DIR).join(PROGRAM);
    let mut command = Command::new(&program);
    // Naming inputs 0 and 1 explicitly gives a smoother animation than the directory mode.
    // -u is UHD mode, -x spatial and -z temporal test-time augmentation.
    command
        .args(["-m", model, "-0"])
        .arg(first)
        .arg("-1")
        .arg(second)
        .arg("-o")
        .arg(between)
        .args(["-u", "-x", "-z"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut status = None;
    for _ in 0..ATTEMPTS {
        let attempt = command
            .status()
            .with_context(|| format!("running {}", program.display()))?;
        if attempt.success() {
            return Ok(());
        }
        status = Some(attempt);
    }
    bail!(
        "{PROGRAM} failed {ATTEMPTS} times on {} and {}: {}",
        first.display(),
        second.display(),
        status.map(|status| status.to_string()).unwrap_or_default()
    )
}
