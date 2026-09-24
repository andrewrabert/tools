use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::tools::preview::config::{Caching, Color, Config, Headers};

#[derive(ClapArgs)]
#[command(about = "Preview files with syntax highlighting and metadata extraction")]
pub struct Preview {
    #[arg(long, help = "label every preview with its file name")]
    full: bool,
    #[arg(long, help = "disable color output")]
    no_color: bool,
    #[arg(long, help = "render the preview instead of asking pathcacher")]
    no_cache: bool,
    #[arg(value_name = "FILE", required = true)]
    files: Vec<PathBuf>,
}

impl From<Preview> for Config {
    fn from(args: Preview) -> Self {
        let color = if args.no_color {
            Color::Never
        } else {
            Color::Always
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
