use std::io::{self, IsTerminal};
use std::path::PathBuf;

use clap::{Args as ClapArgs, ValueEnum};

use crate::tools::preview::config::{Caching, Color, Config, Headers};

#[derive(ClapArgs)]
#[command(about = "Preview files with syntax highlighting and metadata extraction")]
pub struct Preview {
    #[arg(long, help = "label every preview with its file name")]
    full: bool,
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto, help = "when to color output")]
    color: ColorChoice,
    #[arg(long, help = "render the preview instead of asking pathcacher")]
    no_cache: bool,
    #[arg(value_name = "FILE", required = true)]
    files: Vec<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl From<Preview> for Config {
    fn from(args: Preview) -> Self {
        let color = match args.color {
            ColorChoice::Auto if io::stdout().is_terminal() => Color::Always,
            ColorChoice::Auto | ColorChoice::Never => Color::Never,
            ColorChoice::Always => Color::Always,
        };
        let headers = if args.full {
            Headers::Full
        } else if args.files.len() > 1 {
            Headers::Multiple
        } else {
            Headers::Omitted
        };
        let caching = if args.no_cache {
            Caching::Bypass
        } else {
            Caching::Pathcacher
        };
        Config {
            files: args.files,
            color,
            headers,
            caching,
        }
    }
}
