use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;

use crate::tools::Tool;
use crate::walk;

const VALID_PUNCTUATION: &[u8] = b" .,-_()[]'";

#[derive(ClapArgs)]
#[command(about = "Strip characters that are invalid on Windows from file paths")]
pub struct EscapeFilesWindows {
    #[arg(short, long, help = "commit renames to disk. otherwise dry run")]
    commit: bool,
    #[arg(
        value_name = "PATH",
        required = true,
        help = "files or directories to escape"
    )]
    paths: Vec<PathBuf>,
}

struct Rename {
    from: PathBuf,
    to: PathBuf,
}

impl Tool for EscapeFilesWindows {
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

fn run(args: EscapeFilesWindows) -> Result<()> {
    for root in args.paths {
        // Plan every rename under a root before touching disk, so errors surface first.
        for Rename { from, to } in plan(&root)? {
            if args.commit {
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                fs::rename(&from, &to)
                    .with_context(|| format!("failed to rename {}", from.display()))?;
            }
            println!("{}\n{}\n", from.display(), to.display());
        }
    }
    Ok(())
}

fn plan(root: &Path) -> Result<Vec<Rename>> {
    let mut renames = Vec::new();
    let mut claimed = HashSet::new();
    for from in all_files(root)? {
        let to = escape_path(&from)?;
        if to == from {
            continue;
        }
        if fs::symlink_metadata(&to).is_ok() || !claimed.insert(to.clone()) {
            bail!("\"{}\" already exists", to.display());
        }
        renames.push(Rename { from, to });
    }
    Ok(renames)
}

fn all_files(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        bail!("path does not exist: {}", root.display());
    }
    let mut files = Vec::new();
    for entry in walk::entries(root) {
        let entry = entry?;
        if entry.file_type().is_file() {
            files.push(entry.into_path());
        }
    }
    files.sort();
    Ok(files)
}

fn escape_path(path: &Path) -> Result<PathBuf> {
    let mut escaped = PathBuf::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            escaped.push(component);
            continue;
        };
        let part = escape_part(part);
        if part.is_empty() || part == "." || part == ".." {
            bail!("unable to escape {}", path.display());
        }
        escaped.push(part);
    }
    Ok(escaped)
}

fn escape_part(part: &OsStr) -> OsString {
    let bytes = part
        .as_bytes()
        .iter()
        .copied()
        .filter(|byte| byte.is_ascii_alphanumeric() || VALID_PUNCTUATION.contains(byte))
        .collect();
    OsString::from_vec(bytes)
}
