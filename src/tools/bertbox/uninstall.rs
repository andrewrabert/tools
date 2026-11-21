use std::process::ExitCode;

use clap::Args as ClapArgs;
use clap::builder::PossibleValuesParser;

use crate::tools::Tool;
use crate::tools::bertbox::packages;

#[derive(ClapArgs)]
#[command(about = "Uninstall packages")]
pub struct Uninstall {
    /// Package to uninstall (repeatable)
    #[arg(
        value_name = "PACKAGE",
        required = true,
        value_parser = PossibleValuesParser::new(packages::names())
    )]
    packages: Vec<String>,
}

impl Tool for Uninstall {
    fn run(self) -> ExitCode {
        packages::run_each(&self.packages, |package, host| {
            if !package.uninstall(package.selected_target(host)?)? {
                println!("{} is not installed", package.name);
            }
            Ok(())
        })
    }
}
