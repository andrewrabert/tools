use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::source::Source;

#[derive(ClapArgs)]
#[command(about = "Dump and restore music tags to/from JSON")]
pub struct AudioTag {
    #[arg(
        short = 'i',
        long,
        value_name = "FILE",
        help = "input JSON file for replace/merge (use \"-\" for stdin)"
    )]
    pub input: Option<Source>,
    #[arg(
        short = 'o',
        long,
        value_name = "FILE",
        help = "output JSON file for dump (default: stdout)"
    )]
    pub output: Option<PathBuf>,
    #[arg(
        long,
        help = "merge mode: only update/delete specified tags (requires -i)"
    )]
    pub merge: bool,
    #[arg(
        short = 'S',
        long,
        help = "structured output: group by directory with album/albumartist at top level"
    )]
    pub structured: bool,
    #[arg(
        short = 's',
        long = "set",
        num_args = 2,
        value_names = ["TAG", "VALUE"],
        help = "set tag to value (can be specified multiple times)"
    )]
    pub set: Vec<String>,
    #[arg(
        short = 'd',
        long,
        value_name = "TAG",
        help = "delete tag (can be specified multiple times)"
    )]
    pub delete: Vec<String>,
    #[arg(
        short = 'T',
        long,
        help = "launch interactive TUI for browsing and editing tags"
    )]
    pub tui: bool,
    #[arg(
        value_name = "PATH",
        help = "music file(s) or directory for dump/set mode"
    )]
    pub paths: Vec<PathBuf>,
}
