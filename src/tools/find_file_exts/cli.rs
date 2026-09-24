use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::tools::find_file_exts::config::{Config, Folding, Output};

#[derive(ClapArgs)]
#[command(about = "List the file extensions found under paths or named on stdin")]
pub struct FindFileExts {
    #[arg(short, long, help = "treat extensions that differ only by case as one")]
    ignore_case: bool,
    #[command(flatten)]
    output: OutputArgs,
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
}

#[derive(ClapArgs)]
#[group(multiple = false)]
struct OutputArgs {
    #[arg(short, long, help = "print how many files have each extension")]
    count: bool,
    #[arg(short, long, help = "print the files of each extension as json")]
    json: bool,
}

impl From<FindFileExts> for Config {
    fn from(args: FindFileExts) -> Self {
        let output = match args.output {
            OutputArgs { count: true, .. } => Output::Counts,
            OutputArgs { json: true, .. } => Output::Json,
            OutputArgs { .. } => Output::Suffixes,
        };
        let folding = if args.ignore_case {
            Folding::Lowercase
        } else {
            Folding::Preserve
        };
        Config {
            roots: args.paths,
            folding,
            output,
        }
    }
}
