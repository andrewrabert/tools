use std::ffi::OsString;

use clap::Args as ClapArgs;

use crate::tools::full_update::script::ScriptName;

#[derive(ClapArgs)]
#[command(about = "Run every full-update script of every dotfiles directory")]
pub struct FullUpdate {
    #[arg(
        long,
        value_name = "NAME",
        help = "skip script by name (can be repeated)"
    )]
    pub skip: Vec<ScriptName>,
    #[arg(value_name = "SCRIPT", help = "run only this script")]
    pub script: Option<ScriptName>,
    #[arg(
        value_name = "ARG",
        trailing_var_arg = true,
        allow_hyphen_values = true,
        help = "arguments passed to SCRIPT"
    )]
    pub script_args: Vec<OsString>,
}
