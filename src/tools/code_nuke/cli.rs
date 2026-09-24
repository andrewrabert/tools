use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand};

#[derive(ClapArgs)]
#[command(about = "Delete Rust code in bulk: statements around a pattern, tests, or comments")]
pub struct CodeNuke {
    #[command(subcommand)]
    pub target: Target,
}

#[derive(Subcommand)]
pub enum Target {
    #[command(
        about = "delete the innermost node of a given kind containing a pattern",
        after_help = "discover a kind with: ast-grep run -l rust -p 'if $C { $$$B }' --debug-query=ast FILE"
    )]
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
        #[command(flatten)]
        passthrough: Passthrough,
    },
    #[command(about = "delete #[test] fns and #[cfg(test)] mods, attributes included")]
    Tests {
        #[arg(help = "path to scan")]
        path: PathBuf,
        #[command(flatten)]
        passthrough: Passthrough,
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
    #[arg(
        short,
        long,
        help = "rewrite the files; without it the diff is printed and nothing changes"
    )]
    pub apply: bool,
    #[arg(
        value_name = "ARG",
        trailing_var_arg = true,
        allow_hyphen_values = true,
        help = "further arguments passed through to the underlying tool"
    )]
    pub rest: Vec<OsString>,
}
