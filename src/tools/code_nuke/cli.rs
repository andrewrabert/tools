use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand};

const APPLY_HELP: &str = "rewrite the files; without it the diff is printed and nothing changes";

#[derive(ClapArgs)]
#[command(about = "Delete Rust code in bulk: statements around a pattern, tests, or comments")]
pub struct CodeNuke {
    #[command(subcommand)]
    pub target: Target,
}

#[derive(Subcommand)]
pub enum Target {
    #[command(about = "delete the innermost node of a given kind containing a pattern")]
    Stmt {
        #[arg(help = "path to scan")]
        directory: PathBuf,
        #[arg(help = "ast-grep pattern, e.g. 'Settings::resolve($$$A)'")]
        pattern: String,
        #[arg(
            short,
            long,
            value_delimiter = ',',
            default_value = "let_declaration,expression_statement",
            help = "enclosing node kinds to delete, comma separated"
        )]
        kinds: Vec<String>,
        #[arg(short, long, help = APPLY_HELP)]
        apply: bool,
    },
    #[command(about = "delete #[test] fns and #[cfg(test)] mods, attributes included")]
    Tests {
        #[arg(help = "path to scan")]
        path: PathBuf,
        #[arg(short, long, help = APPLY_HELP)]
        apply: bool,
    },
    #[command(about = "print the node kinds around a line, innermost first")]
    Kinds {
        #[arg(help = "Rust file to inspect")]
        file: PathBuf,
        #[arg(help = "line number, starting at 1")]
        line: NonZeroUsize,
    },
    #[command(about = "delete every comment, TODO, FIXME and doc comment included")]
    Comments {
        #[arg(help = "path to scan")]
        path: PathBuf,
        #[command(flatten)]
        passthrough: Passthrough,
    },
}

#[derive(ClapArgs)]
pub struct Passthrough {
    #[arg(short, long, help = APPLY_HELP)]
    pub apply: bool,
    #[arg(
        value_name = "ARG",
        trailing_var_arg = true,
        allow_hyphen_values = true,
        help = "further arguments passed through to the underlying tool"
    )]
    pub rest: Vec<OsString>,
}
