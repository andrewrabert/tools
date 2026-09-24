use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use crossterm::style::{ResetColor, SetBackgroundColor};
use qrcode::{Color, EcLevel, QrCode};

use crate::tools::Tool;

// The output of `qrencode -t ANSI`: error correction level L, a margin of 4 modules, and each
// module two spaces wide on a white or black background, switching color only on a change.
const MARGIN: usize = 4;
const MODULE: &str = "  ";
const LIGHT: SetBackgroundColor = SetBackgroundColor(crossterm::style::Color::Grey);
const DARK: SetBackgroundColor = SetBackgroundColor(crossterm::style::Color::Black);
const RESET: ResetColor = ResetColor;

#[derive(ClapArgs)]
#[command(about = "Print a QR code of an argument or stdin to the terminal")]
pub struct Qr {
    #[arg(value_name = "TEXT")]
    text: Option<OsString>,
}

impl Tool for Qr {
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

fn run(args: Qr) -> Result<()> {
    let data = match args.text {
        Some(text) => text.into_vec(),
        None => {
            let mut data = Vec::new();
            io::stdin().lock().read_to_end(&mut data)?;
            data
        }
    };
    if data.is_empty() {
        bail!("no input data");
    }
    let code =
        QrCode::with_error_correction_level(&data, EcLevel::L).context("encoding the QR code")?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(render(&code).as_bytes())?;
    stdout.flush()?;
    Ok(())
}

fn render(code: &QrCode) -> String {
    let width = code.width();
    let colors = code.to_colors();
    let margin_line = format!("{LIGHT}{}{RESET}\n", MODULE.repeat(width + 2 * MARGIN));
    let side = MODULE.repeat(MARGIN);

    let mut out = margin_line.repeat(MARGIN);
    for row in colors.chunks(width) {
        write!(out, "{LIGHT}{side}").unwrap();
        let mut dark = false;
        for color in row {
            let is_dark = *color == Color::Dark;
            if is_dark != dark {
                write!(out, "{}", if is_dark { DARK } else { LIGHT }).unwrap();
                dark = is_dark;
            }
            out.push_str(MODULE);
        }
        if dark {
            write!(out, "{LIGHT}").unwrap();
        }
        writeln!(out, "{side}{RESET}").unwrap();
    }
    out.push_str(&margin_line.repeat(MARGIN));
    out
}
