use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::tools::pathbin::config::{Config, Grouping};

#[derive(ClapArgs)]
#[command(about = "List executables in path")]
pub struct Pathbin {
    #[command(flatten)]
    grouping: GroupingArgs,
    #[arg(value_name = "PATH", help = "paths to search (default: $PATH)")]
    paths: Vec<PathBuf>,
}

#[derive(ClapArgs)]
#[group(multiple = false)]
struct GroupingArgs {
    #[arg(
        short = 'c',
        help = "group by command. directories are ordered as they appear in the path list"
    )]
    command: bool,
    #[arg(short = 'd', help = "group by directory")]
    directory: bool,
}

impl From<Pathbin> for Config {
    fn from(args: Pathbin) -> Self {
        let grouping = match args.grouping {
            GroupingArgs { command: true, .. } => Grouping::ByCommand,
            GroupingArgs {
                directory: true, ..
            } => Grouping::ByDirectory,
            GroupingArgs { .. } => Grouping::Flat,
        };
        Config {
            dirs: args.paths,
            grouping,
        }
    }
}
