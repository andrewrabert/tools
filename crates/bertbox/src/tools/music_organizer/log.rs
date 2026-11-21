use std::env;
use std::fs::File;
use std::io::{self, Write};

use anyhow::{Context, Result};

const LOG_NAME: &str = "music-organizer.log";

/// Messages go to stderr and to a fresh log file in the temporary directory.
pub struct Log {
    file: File,
}

impl Log {
    pub fn open() -> Result<Self> {
        let path = env::temp_dir().join(LOG_NAME);
        let file = File::create(&path).with_context(|| format!("creating {}", path.display()))?;
        Ok(Self { file })
    }

    pub fn info(&mut self, message: impl AsRef<str>) {
        self.write("INFO", message.as_ref());
    }

    pub fn error(&mut self, message: impl AsRef<str>) {
        self.write("ERROR", message.as_ref());
    }

    fn write(&mut self, level: &str, message: &str) {
        let line = format!("{level} {message}\n");
        let _ = io::stderr().write_all(line.as_bytes());
        let _ = self.file.write_all(line.as_bytes());
    }
}
