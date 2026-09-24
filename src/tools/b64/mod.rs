mod cli;

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::Result;
use anyhow::bail;
use base64::Engine;
use base64::alphabet;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::STANDARD;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};

use crate::input;
use crate::tools::Tool;
pub use crate::tools::b64::cli::B64d;
pub use crate::tools::b64::cli::B64e;

const DECODER: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::RequireCanonical),
);

impl Tool for B64d {
    fn run(self) -> ExitCode {
        match decode(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

impl Tool for B64e {
    fn run(self) -> ExitCode {
        match encode(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn decode(args: B64d) -> Result<()> {
    let data = input::read(args.text)?;
    let data = data.trim_ascii();
    let decoded = peel(data);
    if decoded == data {
        bail!("cannot decode");
    }
    write(&decoded)
}

fn encode(args: B64e) -> Result<()> {
    let data = input::read(Vec::from_iter(args.text))?;
    write(STANDARD.encode(data).as_bytes())
}

fn peel(data: &[u8]) -> Vec<u8> {
    let mut value = data.to_vec();
    while !value.is_empty() {
        match DECODER.decode(&value) {
            Ok(inner) => value = inner,
            Err(_) => break,
        }
    }
    value
}

fn write(bytes: &[u8]) -> Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(bytes)?;
    stdout.flush()?;
    Ok(())
}
