use std::process::ExitCode;

use clap::Args as ClapArgs;

use crate::tools::Tool;
use crate::tools::bertbox::each::each_package;
use crate::tools::bertbox::packages::{self, Removal, Source};

#[derive(ClapArgs)]
#[command(about = "Uninstall packages")]
pub struct Uninstall {
    /// Package to uninstall (repeatable)
    #[arg(value_name = "PACKAGE", required = true, value_parser = packages::parser())]
    packages: Vec<&'static Source>,
}

impl Tool for Uninstall {
    fn run(self) -> ExitCode {
        each_package(&self.packages, |source, host| {
            if source.uninstall(source.selected_target(host)?)? == Removal::NotInstalled {
                println!("{} is not installed", source.name);
            }
            Ok(())
        })
    }
}
