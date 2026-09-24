//! clap argument definitions.
use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand};

#[derive(ClapArgs)]
#[command(about = "Check, format, print, and list cue sheets")]
pub struct CueTool {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "check cue sheets and that each file a sheet lists exists")]
    Check(Check),
    #[command(about = "print cue sheets in a uniform layout")]
    Fmt(Fmt),
    #[command(about = "print the tokens of cue sheets as JSON, pretty-printed on a terminal")]
    Json(Json),
    #[command(about = "print the absolute path of each file cue sheets list")]
    Files(Files),
}

#[derive(ClapArgs)]
pub struct Input {
    #[arg(
        value_name = "PATH",
        help = "a cue sheet, or a directory with --recursive; stdin when absent"
    )]
    pub path: Option<PathBuf>,
    #[arg(
        short,
        long,
        requires = "path",
        help = "search the directory PATH for .cue files"
    )]
    pub recursive: bool,
}

#[derive(ClapArgs)]
pub struct Check {
    #[command(flatten)]
    pub input: Input,
    #[arg(
        short,
        long,
        help = "check that each cue sheet parses; listed files are not checked"
    )]
    pub parse_only: bool,
    #[arg(
        long,
        requires = "path",
        help = "rewrite each cue sheet that is not UTF-8 without a byte-order mark as UTF-8"
    )]
    pub fix_encoding: bool,
    #[arg(long, help = "exit zero even when a cue sheet has errors")]
    pub exit_zero: bool,
}

#[derive(ClapArgs)]
pub struct Fmt {
    #[command(flatten)]
    pub input: Input,
    #[arg(
        short,
        long,
        requires = "path",
        required_if_eq("recursive", "true"),
        help = "rewrite each cue sheet instead of printing it; required by --recursive"
    )]
    pub in_place: bool,
}

#[derive(ClapArgs)]
pub struct Json {
    #[command(flatten)]
    pub input: Input,
}

#[derive(ClapArgs)]
pub struct Files {
    #[command(flatten)]
    pub input: Input,
    #[arg(short = '0', help = "separate paths with NUL instead of newline")]
    pub null: bool,
}
