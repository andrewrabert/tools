use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::mime;
use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "Print the MIME type of files by their content")]
pub struct Mime {
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
}

impl Tool for Mime {
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

fn run(args: Mime) -> Result<()> {
    for path in &args.paths {
        println!("{}", mime::detect(path)?);
    }
    Ok(())
}
