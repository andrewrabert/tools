mod api;
mod cli;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::tools::Tool;
use crate::tools::immich_remote::api::{Api, Asset};
use crate::tools::immich_remote::cli::Command;
pub use crate::tools::immich_remote::cli::ImmichRemote;

impl Tool for ImmichRemote {
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

fn run(args: ImmichRemote) -> Result<()> {
    let api = Api::new(&args.url, &args.api_key)?;
    match args.command {
        Command::EmptyTrash {
            dryrun,
            missing_only,
        } => empty_trash(&api, &existing(args.parents), dryrun, missing_only),
        Command::RestoreTrash { dryrun } => restore_trash(&api, &existing(args.parents), dryrun),
        Command::Scan => scan(&api),
    }
}

/// The given parents that exist as directories on this machine.
fn existing(parents: Vec<PathBuf>) -> Vec<PathBuf> {
    parents
        .into_iter()
        .filter(|parent| parent.is_dir())
        .collect()
}

/// Whether the asset lies under a parent, compared by whole path components.
fn under_valid_parent(asset: &Asset, parents: &[PathBuf]) -> bool {
    let path = Path::new(&asset.original_path);
    parents.iter().any(|parent| path.starts_with(parent))
}

fn empty_trash(api: &Api, parents: &[PathBuf], dryrun: bool, missing_only: bool) -> Result<()> {
    for asset in api.trash_assets()? {
        let path = Path::new(&asset.original_path);

        // Prevent unintentional deletion of media:
        // 2025-06-02 immich somehow trashed the entire library.
        if asset.is_offline && !missing_only {
            println!("warning: skipping offline item {}", path.display());
            continue;
        }

        if !missing_only && !under_valid_parent(&asset, parents) {
            continue;
        }
        if path.is_file() {
            if missing_only {
                println!("warning: path exists, skipping removal {}", path.display());
                continue;
            }
            println!("unlinking {}", path.display());
            if !dryrun {
                fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
            }
        }
        println!("deleting {} {}", path.display(), asset.id);
        if !dryrun {
            api.delete_assets(&[asset.id.as_str()])?;
        }
    }
    Ok(())
}

fn restore_trash(api: &Api, parents: &[PathBuf], dryrun: bool) -> Result<()> {
    for asset in api.trash_assets()? {
        let path = Path::new(&asset.original_path);

        // Prevent unintentional restoration of media:
        // 2025-06-02 immich somehow trashed the entire library.
        if asset.is_offline {
            println!("warning: skipping offline item {}", path.display());
            continue;
        }

        if !under_valid_parent(&asset, parents) {
            continue;
        }
        if path.is_file() {
            println!("restore {} {}", path.display(), asset.id);
            if !dryrun {
                api.restore_assets(&[asset.id.as_str()])?;
            }
        } else {
            println!(
                "warning: skipping restore, path not found {}",
                path.display()
            );
        }
    }
    Ok(())
}

fn scan(api: &Api) -> Result<()> {
    for library in api.libraries()? {
        println!("scanning library {}", library.id);
        api.scan_library(&library.id)?;
    }
    Ok(())
}
