use std::process::ExitCode;

use clap::Args as ClapArgs;

use crate::git::{self, Git};
use crate::tools::Tool;
use crate::tools::git::exit_code;

#[derive(ClapArgs)]
#[command(about = "List tags with their creation dates")]
pub struct TagTimestamp {}

impl Tool for TagTimestamp {
    fn run(self) -> ExitCode {
        exit_code(git::run(&mut Git::cwd().command([
            "tag",
            "-l",
            "--format=%(refname:short) %(creatordate:iso8601)",
        ])))
    }
}
