use std::fmt;
use std::str::FromStr;

use anyhow::bail;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Topic(String);

impl FromStr for Topic {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains(['/', '\0']) {
            bail!("topic must not contain '/' or NUL");
        }
        Ok(Topic(s.to_owned()))
    }
}

impl fmt::Display for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
