use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;

use anyhow::{Context, Result};

#[derive(Clone)]
pub enum Source {
    Stdin,
    File(PathBuf),
}

impl From<OsString> for Source {
    fn from(value: OsString) -> Self {
        if value == "-" {
            Source::Stdin
        } else {
            Source::File(PathBuf::from(value))
        }
    }
}

impl Source {
    pub fn open(&self) -> Result<Box<dyn Read>> {
        match self {
            Source::Stdin => Ok(Box::new(io::stdin().lock())),
            Source::File(path) => {
                let file =
                    File::open(path).with_context(|| format!("opening {}", path.display()))?;
                Ok(Box::new(file))
            }
        }
    }
}
