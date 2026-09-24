use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use reqwest::blocking::Client;

use crate::tools::Tool;
use crate::{APP_NAME, APP_VERSION};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IPV4_URL: &str = "https://ipv4.icanhazip.com";
const IPV6_URL: &str = "https://ipv6.icanhazip.com";

#[derive(ClapArgs)]
#[command(about = "Print the public IPv4 address")]
pub struct PublicIpv4 {}

#[derive(ClapArgs)]
#[command(about = "Print the public IPv6 address")]
pub struct PublicIpv6 {}

impl Tool for PublicIpv4 {
    fn run(self) -> ExitCode {
        exit_code(run(IPV4_URL))
    }
}

impl Tool for PublicIpv6 {
    fn run(self) -> ExitCode {
        exit_code(run(IPV6_URL))
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

fn run(url: &str) -> Result<()> {
    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .user_agent(format!("{APP_NAME}/{APP_VERSION}"))
        .build()
        .context("building the http client")?;
    let address = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .with_context(|| format!("requesting {url}"))?
        .bytes()
        .with_context(|| format!("reading the response of {url}"))?;

    let mut stdout = io::stdout().lock();
    stdout.write_all(&address)?;
    stdout.flush()?;
    Ok(())
}
