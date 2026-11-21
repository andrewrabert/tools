use std::process::ExitCode;

use clap::Args as ClapArgs;

use crate::tools::Tool;
use crate::tools::bertbox::each::each_package;
use crate::tools::bertbox::packages::{self, Package, Removal};

#[derive(ClapArgs)]
#[command(about = "Uninstall packages")]
pub struct Uninstall {
    /// Package to uninstall (repeatable)
    #[arg(value_name = "PACKAGE", required = true, value_parser = packages::parser())]
    packages: Vec<&'static Package>,
}

impl Tool for Uninstall {
    fn run(self) -> ExitCode {
        each_package(&self.packages, |package, host| {
            if package.uninstall(package.selected_target(host)?)? == Removal::NotInstalled {
                println!("{} is not installed", package.name);
            }
            Ok(())
        })
    }
}
