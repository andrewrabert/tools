use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use chardetng::EncodingDetector;
use clap::Args as ClapArgs;

use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "Reencode files in UTF-8")]
pub struct EncodeUtf8 {
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
}

impl Tool for EncodeUtf8 {
    fn run(self) -> ExitCode {
        for path in &self.paths {
            if let Err(error) = reencode(path) {
                eprintln!("Error: {error:?}");
                return ExitCode::FAILURE;
            }
        }
        ExitCode::SUCCESS
    }
}

fn reencode(path: &Path) -> Result<()> {
    let data = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut detector = EncodingDetector::new();
    detector.feed(&data, true);
    let encoding = detector.guess(None, true);
    let (text, _, malformed) = encoding.decode(&data);
    if malformed {
        bail!("{} is not valid {}", path.display(), encoding.name());
    }
    if text.as_bytes() == data {
        println!("Already encoded {}", path.display());
    } else {
        println!("Encoding {}", path.display());
        fs::write(path, text.as_bytes()).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}
