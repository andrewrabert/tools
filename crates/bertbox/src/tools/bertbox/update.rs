use std::process::ExitCode;

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::install_target::InstallTarget;
use crate::tools::Tool;
use crate::tools::bertbox::each::each_package;
use crate::tools::bertbox::packages::{self, Source};

#[derive(ClapArgs)]
#[command(about = "Update installed packages")]
pub struct Update {
    /// Package to update (repeatable) [default: every installed package]
    #[arg(value_name = "PACKAGE", value_parser = packages::parser())]
    packages: Vec<&'static Source>,
}

impl Tool for Update {
    fn run(self) -> ExitCode {
        if self.packages.is_empty() {
            let all: Vec<&'static Source> = packages::all().iter().collect();
            return each_package(&all, |source, host| update(source, host, false));
        }
        each_package(&self.packages, |source, host| update(source, host, true))
    }
}

fn update(source: &Source, host: InstallTarget, named: bool) -> Result<()> {
    if source.installed(host)? {
        return source.install(source.selected_target(host)?, false);
    }
    if named {
        println!("{} is not installed", source.name);
    }
    Ok(())
}
