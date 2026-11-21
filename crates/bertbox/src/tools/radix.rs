use std::fmt::{self, Display, Formatter, Write};
use std::iter;
use std::process::ExitCode;
use std::str::FromStr;

use anyhow::{Context, Error, Result, bail};
use clap::Args as ClapArgs;

use crate::tools::Tool;

const BITS_PER_NIBBLE: usize = 4;

#[derive(Clone)]
struct HexDigits(Vec<u8>);

#[derive(Clone)]
struct BinaryDigits(Vec<bool>);

#[derive(ClapArgs)]
#[command(about = "Convert binary digits to hexadecimal")]
pub struct Binary2hex {
    #[arg(value_name = "BIN")]
    bin: BinaryDigits,
}

#[derive(ClapArgs)]
#[command(about = "Convert hexadecimal digits to binary")]
pub struct Hex2binary {
    #[arg(value_name = "HEX")]
    hex: HexDigits,
}

impl Tool for Binary2hex {
    fn run(self) -> ExitCode {
        println!("{}", HexDigits::from(self.bin));
        ExitCode::SUCCESS
    }
}

impl Tool for Hex2binary {
    fn run(self) -> ExitCode {
        println!("{}", BinaryDigits::from(self.hex));
        ExitCode::SUCCESS
    }
}

impl FromStr for HexDigits {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        if text.is_empty() {
            bail!("expected at least one hexadecimal digit");
        }
        let mut nibbles = Vec::with_capacity(text.len());
        for c in text.chars() {
            let nibble = c
                .to_digit(16)
                .with_context(|| format!("not a hexadecimal digit: {c:?}"))?;
            nibbles.push(u8::try_from(nibble)?);
        }
        Ok(HexDigits(nibbles))
    }
}

impl FromStr for BinaryDigits {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        if text.is_empty() {
            bail!("expected at least one binary digit");
        }
        let mut bits = Vec::with_capacity(text.len());
        for c in text.chars() {
            match c {
                '0' => bits.push(false),
                '1' => bits.push(true),
                _ => bail!("not a binary digit: {c:?}"),
            }
        }
        Ok(BinaryDigits(bits))
    }
}

impl From<HexDigits> for BinaryDigits {
    fn from(hex: HexDigits) -> Self {
        let mut bits = Vec::with_capacity(hex.0.len() * BITS_PER_NIBBLE);
        for nibble in hex.0 {
            for shift in (0..BITS_PER_NIBBLE).rev() {
                bits.push(nibble >> shift & 1 == 1);
            }
        }
        BinaryDigits(bits)
    }
}

impl From<BinaryDigits> for HexDigits {
    fn from(binary: BinaryDigits) -> Self {
        let padding = (BITS_PER_NIBBLE - binary.0.len() % BITS_PER_NIBBLE) % BITS_PER_NIBBLE;
        let bits: Vec<bool> = iter::repeat_n(false, padding).chain(binary.0).collect();
        let nibbles = bits
            .chunks(BITS_PER_NIBBLE)
            .map(|chunk| {
                chunk
                    .iter()
                    .fold(0, |nibble, &bit| nibble << 1 | u8::from(bit))
            })
            .collect();
        HexDigits(nibbles)
    }
}

impl Display for HexDigits {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for nibble in &self.0 {
            write!(f, "{nibble:x}")?;
        }
        Ok(())
    }
}

impl Display for BinaryDigits {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for &bit in &self.0 {
            f.write_char(if bit { '1' } else { '0' })?;
        }
        Ok(())
    }
}
