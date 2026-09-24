use std::io::{self, IsTerminal, Read, Write};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow};
use clap::Args as ClapArgs;
use reqwest::Url;
use reqwest::blocking::Client;
use serde_json::Value;

use crate::tools::Tool;
use crate::{APP_NAME, APP_VERSION};

const ENDPOINT: &str = "http://ip-api.com/json";

#[derive(ClapArgs)]
#[command(about = "Look up the geolocation of addresses from arguments or stdin")]
pub struct Geoip {
    #[arg(value_name = "ADDRESS")]
    addresses: Vec<String>,
}

impl Tool for Geoip {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: Geoip) -> Result<()> {
    let mut stdin = io::stdin().lock();
    let addresses = if stdin.is_terminal() {
        args.addresses
    } else {
        let mut data = String::new();
        stdin.read_to_string(&mut data)?;
        data.split_whitespace().map(str::to_owned).collect()
    };

    let client = Client::builder()
        .user_agent(format!("{APP_NAME}/{APP_VERSION}"))
        .build()
        .context("building the http client")?;
    let mut stdout = io::stdout().lock();
    for address in &addresses {
        let location = lookup(&client, address)?;
        // serde_json keeps object keys sorted, matching `jq -S`.
        serde_json::to_writer_pretty(&mut stdout, &location)?;
        writeln!(stdout)?;
    }
    stdout.flush()?;
    Ok(())
}

fn lookup(client: &Client, address: &str) -> Result<Value> {
    let mut url = Url::parse(ENDPOINT).context("parsing the geoip endpoint")?;
    url.path_segments_mut()
        .map_err(|()| anyhow!("geoip endpoint cannot hold a path"))?
        .push(address);
    let body = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .with_context(|| format!("looking up {address}"))?
        .bytes()
        .with_context(|| format!("reading the lookup of {address}"))?;
    serde_json::from_slice(&body).with_context(|| format!("parsing the lookup of {address}"))
}
