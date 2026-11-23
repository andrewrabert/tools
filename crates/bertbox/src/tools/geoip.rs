use std::io::{self, IsTerminal, Read, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use bertbox_ipapi::Api;
use clap::Args as ClapArgs;

use crate::tools::Tool;

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

    let api = Api::new()?;
    let mut stdout = io::stdout().lock();
    for address in &addresses {
        let location = api
            .lookup(address)
            .with_context(|| format!("looking up {address}"))?;
        // serde_json keeps object keys sorted, matching `jq -S`.
        serde_json::to_writer_pretty(&mut stdout, &location)?;
        writeln!(stdout)?;
    }
    stdout.flush()?;
    Ok(())
}
