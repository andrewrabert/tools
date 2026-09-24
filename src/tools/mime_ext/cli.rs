use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::tools::mime_ext::config::{Config, Mode};

#[derive(ClapArgs)]
#[command(about = "Group files by detected MIME type and check or fix their extensions")]
pub struct MimeExt {
    #[command(flatten)]
    mode: ModeArgs,
    #[arg(
        value_name = "PATH",
        required = true,
        help = "files, or directories to walk"
    )]
    paths: Vec<PathBuf>,
}

#[derive(ClapArgs)]
#[group(multiple = false)]
struct ModeArgs {
    #[arg(
        long,
        help = "list only files whose extension does not fit their type; fail if there are any"
    )]
    check: bool,
    #[arg(
        long,
        help = "append the fitting extension to files whose extension does not fit their type"
    )]
    fix: bool,
    #[arg(
        long,
        help = "like --fix, but also give plain text .txt and unrecognized types .unknown"
    )]
    force_fix: bool,
}

impl From<MimeExt> for Config {
    fn from(args: MimeExt) -> Self {
        let mode = match args.mode {
            ModeArgs { check: true, .. } => Mode::Check,
            ModeArgs { fix: true, .. } => Mode::Fix { force: false },
            ModeArgs {
                force_fix: true, ..
            } => Mode::Fix { force: true },
            ModeArgs { .. } => Mode::List,
        };
        Config {
            roots: args.paths,
            mode,
        }
    }
}
