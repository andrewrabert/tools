use std::str::FromStr;

use anyhow::{Error, bail};

#[derive(Clone)]
pub struct Password(String);

impl Password {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Password {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.is_empty() {
            bail!("password cannot be empty");
        }
        Ok(Self(text.to_owned()))
    }
}
