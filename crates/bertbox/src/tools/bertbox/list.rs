use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use serde_json::{Map, Value};

use crate::install_target::{InstallTarget, System};
use crate::tools::Tool;
use crate::tools::bertbox::packages::{self, Package};

#[derive(ClapArgs)]
#[command(about = "List installed packages")]
pub struct List {
    /// Print each installed package's record as JSON, keyed by name
    #[arg(long)]
    json: bool,
}

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
        if self.json {
            let mut records = Map::new();
            for package in installed {
                let installation = match package.installation()? {
                    Some(installation) => serde_json::to_value(installation)
                        .context("rendering the package installation")?,
                    None => Value::Null,
                };
                records.insert(package.name.to_string(), installation);
            }
            let rendered =
                serde_json::to_string_pretty(&records).context("rendering the package records")?;
            writeln!(stdout, "{rendered}")?;
        } else {
            for package in installed {
                writeln!(stdout, "{}", package.name)?;
            }
        }
        stdout.flush()?;
        Ok(())
    }
}

fn installed() -> Result<Vec<&'static Package>> {
    let host = InstallTarget::host()?;
    if host.0 == System::Termux {
        return Ok(Vec::new());
    }
    let mut installed = Vec::new();
    for package in packages::all() {
        if package.installed(host)? {
            installed.push(package);
        }
    }
    Ok(installed)
}
