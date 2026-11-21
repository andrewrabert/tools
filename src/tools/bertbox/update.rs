use std::process::ExitCode;

use anyhow::Result;
use clap::Args as ClapArgs;
use clap::builder::PossibleValuesParser;

use crate::install_target::InstallTarget;
use crate::tools::Tool;
use crate::tools::bertbox::packages::{self, Package};

#[derive(ClapArgs)]
#[command(about = "Update installed packages")]
pub struct Update {
    /// Package to update (repeatable) [default: every installed package]
    #[arg(value_name = "PACKAGE", value_parser = PossibleValuesParser::new(packages::names()))]
    packages: Vec<String>,
}

impl Tool for Update {
    fn run(self) -> ExitCode {
        if self.packages.is_empty() {
            let names: Vec<String> = packages::names().into_iter().map(String::from).collect();
            return packages::run_each(&names, |package, host| update(package, host, false));
        }
        packages::run_each(&self.packages, |package, host| update(package, host, true))
    }
}

fn update(package: &Package, host: InstallTarget, named: bool) -> Result<()> {
    if package.installed(host)? {
        return package.install(package.selected_target(host)?, false);
    }
    if named {
        println!("{} is not installed", package.name);
    }
    Ok(())
}
