use std::process::ExitCode;

use clap::Args as ClapArgs;

use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "Print text as a QR code to the terminal (upstream qrcode)")]
pub struct Qrencode {
    /// Text to encode
    text: String,
}

impl Tool for Qrencode {
    fn run(self) -> ExitCode {
        let code = match ::qrcode::QrCode::new(self.text.as_bytes()) {
            Ok(code) => code,
            Err(error) => {
                eprintln!(
                    "Error: {:?}",
                    anyhow::Error::from(error).context("encoding QR code")
                );
                return ExitCode::FAILURE;
            }
        };
        print!(
            "{}",
            code.render()
                .dark_color("\x1b[7m  \x1b[0m")
                .light_color("\x1b[49m  \x1b[0m")
                .build()
        );
        ExitCode::SUCCESS
    }
}
