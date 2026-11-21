use std::process::ExitCode;

use anyhow::Result;

use crate::install_target::InstallTarget;
use crate::tools::bertbox::packages::Package;

pub fn each_package(
    packages: &[&'static Package],
    action: impl Fn(&Package, InstallTarget) -> Result<()>,
) -> ExitCode {
    let host = match InstallTarget::host() {
        Ok(host) => host,
        Err(error) => {
            eprintln!("Error: {error:?}");
            return ExitCode::FAILURE;
        }
    };
    let mut status = ExitCode::SUCCESS;
    for package in packages {
        if let Err(error) = action(package, host) {
            eprintln!("Error: {}: {error:?}", package.name);
            status = ExitCode::FAILURE;
        }
    }
    status
}
