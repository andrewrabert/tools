use std::process::ExitCode;

use anyhow::Result;

use crate::install_target::InstallTarget;
use crate::tools::bertbox::packages::Source;

pub fn each_package(
    packages: &[&'static Source],
    action: impl Fn(&Source, InstallTarget) -> Result<()>,
) -> ExitCode {
    let host = match InstallTarget::host() {
        Ok(host) => host,
        Err(error) => {
            eprintln!("Error: {error:?}");
            return ExitCode::FAILURE;
        }
    };
    let mut status = ExitCode::SUCCESS;
    for source in packages {
        if let Err(error) = action(source, host) {
            eprintln!("Error: {}: {error:?}", source.name);
            status = ExitCode::FAILURE;
        }
    }
    status
}
