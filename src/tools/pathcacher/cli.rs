use std::ffi::OsString;
use std::path::PathBuf;

use clap::Args as ClapArgs;

#[derive(ClapArgs)]
#[command(about = "Cache a command's stdout against a path's size and mtime")]
pub struct Pathcacher {
    #[arg(long, value_name = "PATH", help = "path the output depends on")]
    pub path: PathBuf,
    #[arg(
        long,
        value_name = "NAME",
        help = "distinguishes commands run on one path"
    )]
    pub name: String,
    #[arg(value_name = "COMMAND")]
    pub command: OsString,
    #[arg(
        value_name = "ARG",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub args: Vec<OsString>,
}
