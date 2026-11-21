use std::process::ExitCode;

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::install_target::InstallTarget;
use crate::tools::Tool;
use crate::tools::bertbox::each::each_package;
use crate::tools::bertbox::packages::{self, Package};

#[derive(ClapArgs)]
#[command(about = "Update installed packages")]
pub struct Update {
    /// Package to update (repeatable) [default: every installed package]
    #[arg(value_name = "PACKAGE", value_parser = packages::parser())]
    packages: Vec<&'static Package>,
}

impl Tool for Update {
    fn run(self) -> ExitCode {
        if self.packages.is_empty() {
            let all: Vec<&'static Package> = packages::all().iter().collect();
            return each_package(&all, |package, host| update(package, host, false));
        }
        each_package(&self.packages, |package, host| update(package, host, true))
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
