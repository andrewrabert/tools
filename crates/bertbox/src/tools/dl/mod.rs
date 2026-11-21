mod cli;
mod fetch;
mod filename;
mod target;

use std::io::{self, IsTerminal};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

use crate::clipboard;
use crate::tools::Tool;
pub use crate::tools::dl::cli::Dl;
use crate::tools::dl::filename::Tls;
use crate::tools::dl::target::{Scheme, Site, Targets};

impl Tool for Dl {
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

fn run(args: Dl) -> Result<()> {
    let mut targets = Targets::default();
    targets.extend(&args.urls, &Scheme::Implied)?;
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        let lines = stdin.lines().collect::<io::Result<Vec<_>>>();
        targets.extend(lines.context("reading stdin")?, &Scheme::Required)?;
    }
    if targets.is_empty() {
        let clipboard = clipboard::paste()?;
        targets.extend(
            String::from_utf8_lossy(&clipboard).lines(),
            &Scheme::Required,
        )?;
    }
    if args.output.is_some() && targets.as_slice().len() != 1 {
        bail!("must specify exactly 1 url when using -o/--output");
    }

    let tls = if args.insecure {
        Tls::Unverified
    } else {
        Tls::Verified
    };
    for target in targets.as_slice() {
        println!("{target}");
        let site = if args.yt {
            Site::YouTube
        } else {
            target.site()
        };
        match (site, &args.output) {
            (Site::YouTube, _) => fetch::yt_dlp(target)?,
            (_, Some(name)) => fetch::direct(target, Some(name), &tls)?,
            (Site::Http, None) => {
                let name = filename::suggested_by_server(target, &tls)?;
                fetch::direct(target, name.as_deref(), &tls)?;
            }
            (Site::Other, None) => fetch::direct(target, None, &tls)?,
        }
    }
    Ok(())
}
