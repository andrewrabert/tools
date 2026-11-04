use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Error};
use clap::Args as ClapArgs;
use minijinja::Value;

#[derive(ClapArgs)]
#[command(about = "Render a Jinja template")]
pub struct Minijinja {
    /// Template string
    #[arg(value_name = "TEMPLATE", conflicts_with = "template")]
    pub template_string: Option<String>,
    /// Template file
    #[arg(
        short,
        long,
        value_name = "FILE",
        required_unless_present = "template_string"
    )]
    pub template: Option<PathBuf>,
    /// File of JSON objects, in place of stdin (repeatable); renders once per object, as `DATA`
    #[arg(short = 'i', long = "input", value_name = "FILE")]
    pub inputs: Vec<PathBuf>,
    /// Define a variable: KEY is true, KEY=VALUE a string, KEY:=VALUE a JSON value
    #[arg(short = 'D', long = "define", value_name = "KEY[=VALUE]")]
    pub defines: Vec<Define>,
    /// Write the output atomically to this file [default: stdout]
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,
}

#[derive(Clone)]
pub struct Define {
    pub name: String,
    pub value: Value,
}

impl FromStr for Define {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (name, value) = if let Some((name, raw)) = s.split_once(":=") {
            let value = serde_json::from_str(raw)
                .with_context(|| format!("invalid value {raw:?} (not JSON)"))?;
            (name, value)
        } else if let Some((name, value)) = s.split_once('=') {
            (name, Value::from(value))
        } else {
            (s, Value::from(true))
        };
        Ok(Define {
            name: name.to_owned(),
            value,
        })
    }
}
