mod cli;
mod session;

use std::process::ExitCode;

use anyhow::Result;

use crate::tools::Tool;
use crate::tools::fanpipe::{self, Message, Topic};
pub use crate::tools::lmk::cli::Lmk;
use crate::tools::lmk::session::Session;

const TOPIC: &str = "lmk";

impl Tool for Lmk {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: Lmk) -> Result<()> {
    let topic: Topic = TOPIC.parse()?;
    if args.command.is_empty() {
        return fanpipe::publish(&topic, Message::resolve(None));
    }
    match Session::detect() {
        Session::Outside => session::host(&args.command),
        Session::Inside(pane) => session::watch(&topic, &pane, &args.command),
    }
}
