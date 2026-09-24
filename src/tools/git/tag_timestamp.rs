use std::process::ExitCode;

use clap::Args as ClapArgs;

use crate::git::Git;
use crate::tools::Tool;
use crate::tools::git::exit_code;

#[derive(ClapArgs)]
#[command(about = "List tags with their creation dates")]
pub struct TagTimestamp {}

impl Tool for TagTimestamp {
    fn run(self) -> ExitCode {
        exit_code(Git::cwd().run([
            "tag",
            "-l",
            "--format=%(refname:short) %(creatordate:iso8601)",
        ]))
    }
}
