use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use clap::Args as ClapArgs;

use crate::tools::find_dupes::config::{Action, Config};

#[derive(ClapArgs)]
#[command(about = "Find duplicate files by content")]
pub struct FindDupes {
    #[arg(long, help = "print what --rm would remove without removing")]
    dryrun: bool,
    #[command(flatten)]
    action: ActionArgs,
    #[arg(value_name = "PATH", default_value = ".")]
    paths: Vec<PathBuf>,
}

#[derive(ClapArgs)]
#[group(multiple = false)]
struct ActionArgs {
    #[arg(short, long, help = "list unique files")]
    unique: bool,
    #[arg(long, help = "remove duplicates (keeps first match)")]
    rm: bool,
    #[arg(long, value_name = "DIR", help = "move duplicates to a directory")]
    mv: Option<PathBuf>,
}

impl TryFrom<FindDupes> for Config {
    type Error = anyhow::Error;

    fn try_from(args: FindDupes) -> Result<Self, Self::Error> {
        let roots = args
            .paths
            .iter()
            .map(|p| fs::canonicalize(p).with_context(|| format!("resolving {}", p.display())))
            .collect::<Result<Vec<_>, _>>()?;
        let action = match args.action {
            ActionArgs { unique: true, .. } => Action::Unique,
            ActionArgs { rm: true, .. } => Action::Remove {
                dry_run: args.dryrun,
            },
            ActionArgs { mv: Some(dir), .. } => Action::Move(dir),
            ActionArgs { .. } => Action::Report,
        };
        Ok(Config { roots, action })
    }
}
