use std::io::{self, Write};
use std::net::IpAddr;
use std::process::ExitCode;

use anyhow::Result;
use bertbox_icanhazip::Api;
use clap::Args as ClapArgs;

use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "Print the public IPv4 address")]
pub struct PublicIpv4 {}

#[derive(ClapArgs)]
#[command(about = "Print the public IPv6 address")]
pub struct PublicIpv6 {}

impl Tool for PublicIpv4 {
    fn run(self) -> ExitCode {
        exit_code(run(|api| Ok(api.ipv4()?.into())))
    }
}

impl Tool for PublicIpv6 {
    fn run(self) -> ExitCode {
        exit_code(run(|api| Ok(api.ipv6()?.into())))
    }
}

fn exit_code(result: Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        }
    }
}

fn run(address: impl FnOnce(&Api) -> Result<IpAddr>) -> Result<()> {
    let api = Api::new()?;
    let address = address(&api)?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{address}")?;
    stdout.flush()?;
    Ok(())
}
