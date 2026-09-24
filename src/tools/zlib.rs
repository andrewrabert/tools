use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::source::Source;
use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "Compress or decompress zlib data")]
pub struct Zlib {
    #[arg(short, long)]
    decompress: bool,
    #[arg(value_name = "FILE", default_value = "-")]
    file: Source,
}

impl Tool for Zlib {
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

fn run(args: Zlib) -> Result<()> {
    let mut input = args.file.open()?;
    let mut stdout = io::stdout().lock();
    if args.decompress {
        io::copy(&mut ZlibDecoder::new(input), &mut stdout).context("decompressing")?;
    } else {
        let mut encoder = ZlibEncoder::new(&mut stdout, Compression::default());
        io::copy(&mut input, &mut encoder).context("compressing")?;
        encoder.finish().context("compressing")?;
    }
    stdout.flush()?;
    Ok(())
}
