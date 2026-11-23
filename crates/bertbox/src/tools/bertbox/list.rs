use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::install_target::{InstallTarget, System};
use crate::tools::Tool;
use crate::tools::bertbox::packages::{self, Source};

#[derive(ClapArgs)]
#[command(about = "List installed packages")]
pub struct List {}

impl Tool for List {
    fn run(self) -> ExitCode {
        match self.list() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

impl List {
    fn list(&self) -> Result<()> {
        let installed = installed()?;
        let mut stdout = io::stdout().lock();
        for source in installed {
            writeln!(stdout, "{}", source.name)?;
        }
        stdout.flush()?;
        Ok(())
    }
}

fn installed() -> Result<Vec<&'static Source>> {
    let host = InstallTarget::host()?;
    if host.0 == System::Termux {
        return Ok(Vec::new());
    }
    let mut installed = Vec::new();
    for source in packages::all() {
        if source.installed(host)? {
            installed.push(source);
        }
    }
    Ok(installed)
}
