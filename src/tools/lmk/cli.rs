use std::ffi::OsString;

use clap::Args as ClapArgs;

#[derive(ClapArgs)]
#[command(about = "Rerun a command whenever lmk is run without one")]
pub struct Lmk {
    #[arg(
        value_name = "COMMAND",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub command: Vec<OsString>,
}
