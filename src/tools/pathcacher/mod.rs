mod cache;
mod cli;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use etcetera::{BaseStrategy, base_strategy::Xdg};

use crate::tools::Tool;
use crate::tools::pathcacher::cache::{Cache, Key};
pub use crate::tools::pathcacher::cli::Pathcacher;

impl Tool for Pathcacher {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: Pathcacher) -> Result<()> {
    let data = cached_output(&args.path, &args.name, || {
        let output = Command::new(&args.command)
            .args(&args.args)
            .stdout(Stdio::piped())
            .spawn()
            .and_then(|child| child.wait_with_output())
            .with_context(|| format!("running {}", args.command.display()))?;
        if !output.status.success() {
            bail!("{} failed: {}", args.command.display(), output.status);
        }
        Ok(output.stdout)
    })?;

    let mut stdout = io::stdout().lock();
    stdout.write_all(&data)?;
    stdout.flush()?;
    Ok(())
}

pub fn cached_output(
    path: &Path,
    name: &str,
    produce: impl FnOnce() -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    let cache = Cache::open(&cache_home()?)?;
    let key = Key::of(path, name)?;
    if let Some(data) = cache.get(&key)? {
        return Ok(data);
    }
    let data = produce()?;
    cache.set(&key, &data)?;
    Ok(data)
}

pub(crate) fn cache_home() -> Result<PathBuf> {
    let strategy = Xdg::new().context("locating the home directory")?;
    Ok(strategy.cache_dir())
}
