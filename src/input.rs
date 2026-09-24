use std::io::{self, IsTerminal, Read};

use anyhow::{Result, bail};

use crate::clipboard;

pub fn read(text: Vec<String>) -> Result<Vec<u8>> {
    let mut stdin = io::stdin().lock();
    if !stdin.is_terminal() {
        if !text.is_empty() {
            bail!("unexpected arguments");
        }
        let mut data = Vec::new();
        stdin.read_to_end(&mut data)?;
        Ok(data)
    } else if text.is_empty() {
        clipboard::paste()
    } else {
        Ok(text.join(" ").into_bytes())
    }
}
