use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;

use crate::chardet;
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
    let text = match chardet::to_utf8(&data) {
        Ok(text) => text,
        Err(error) => bail!("{} is not valid {}", path.display(), error.encoding),
    };
    if text.as_bytes() == data {
        println!("Already encoded {}", path.display());
    } else {
        println!("Encoding {}", path.display());
        fs::write(path, text.as_bytes()).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}
