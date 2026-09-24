use std::ffi::OsString;
use std::process::ExitCode;

use clap::Args as ClapArgs;

use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(
    about = "Search and rewrite code by syntax (upstream ast-grep)",
    disable_help_flag = true,
    disable_version_flag = true
)]
pub struct AstGrep {
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    args: Vec<OsString>,
}

impl Tool for AstGrep {
    fn run(self) -> ExitCode {
        let args = match self
            .args
            .into_iter()
            .map(|arg| {
                arg.into_string()
                    .map_err(|arg| format!("argument is not valid UTF-8: {}", arg.display()))
            })
            .collect::<Result<Vec<String>, String>>()
        {
            Ok(args) => args,
            Err(error) => {
                eprintln!("Error: {error}");
                return ExitCode::FAILURE;
            }
        };
        match ::ast_grep::main_with_args(std::iter::once("ast-grep".to_string()).chain(args)) {
            Ok(code) => code,
            Err(error) => {
                if let Some(error) = error.downcast_ref::<clap::Error>() {
                    error.exit();
                }
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}
