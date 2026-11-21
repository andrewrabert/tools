use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use nix::errno::Errno;

use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "List broken symlinks: the paths given, or those directly inside them")]
pub struct BrokenLinks {
    #[arg(long, help = "remove the links from disk. otherwise only list them")]
    rm: bool,
    #[arg(value_name = "PATH", default_value = ".")]
    paths: Vec<PathBuf>,
}

impl Tool for BrokenLinks {
    fn run(self) -> ExitCode {
        match run(&self.paths, self.rm) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(paths: &[PathBuf], rm: bool) -> Result<()> {
    let mut failed = false;
    for path in paths {
        if path.is_dir() {
            let entries =
                fs::read_dir(path).with_context(|| format!("reading {}", path.display()))?;
            for entry in entries {
                let entry = entry.with_context(|| format!("reading {}", path.display()))?;
                failed |= !handle(&entry.path(), rm);
            }
        } else {
            failed |= !handle(path, rm);
        }
    }
    if failed {
        bail!("some links could not be removed");
    }
    Ok(())
}

/// Lists and optionally removes `path` if it is a broken symlink. False on a failed removal.
fn handle(path: &Path, rm: bool) -> bool {
    if !is_broken_symlink(path) {
        return true;
    }
    println!("{}", path.display());
    if !rm {
        return true;
    }
    match fs::remove_file(path) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("{}: {error}", path.display());
            false
        }
    }
}

/// A symlink whose target cannot be resolved, matching `find -L -type l`.
fn is_broken_symlink(path: &Path) -> bool {
    if !path.is_symlink() {
        return false;
    }
    match fs::metadata(path) {
        Ok(_) => false,
        Err(error) => {
            error.kind() == io::ErrorKind::NotFound
                || error.raw_os_error() == Some(Errno::ELOOP as i32)
        }
    }
}
