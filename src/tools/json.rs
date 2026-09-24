use std::io::Read;
use std::process::ExitCode;
use std::str::FromStr;

use anyhow::{Context, Error, Result, bail};
use clap::Args as ClapArgs;
use regex::{Regex, RegexBuilder};
use serde_json::{Map, Value};

use crate::source::Source;
use crate::tools::Tool;

#[derive(Clone)]
struct Delimiter(String);

#[derive(ClapArgs)]
struct NestingArgs {
    #[arg(value_name = "DELIMITER")]
    delimiter: Delimiter,
    #[arg(value_name = "DATA", default_value = "-")]
    data: Source,
}

#[derive(ClapArgs)]
#[command(about = "Flatten nested json objects into delimiter-joined keys")]
pub struct JsonFlat {
    #[command(flatten)]
    args: NestingArgs,
}

#[derive(ClapArgs)]
#[command(about = "Nest a flat json object by splitting its keys on a delimiter")]
pub struct JsonNest {
    #[command(flatten)]
    args: NestingArgs,
}

#[derive(ClapArgs)]
#[command(about = "Filter a json object of strings by key and value patterns")]
pub struct JsonRe {
    #[arg(long, value_name = "PATTERN", help = "key pattern")]
    key: Option<String>,
    #[arg(long, value_name = "PATTERN", help = "value pattern")]
    value: Option<String>,
    #[arg(short = 'i', help = "ignore case")]
    ignore_case: bool,
    #[arg(short = 'v', help = "invert match")]
    invert: bool,
    #[arg(value_name = "DATA", default_value = "-")]
    data: Source,
}

enum Polarity {
    Matching,
    Inverted,
}

struct Filter {
    key: Option<Regex>,
    value: Option<Regex>,
    polarity: Polarity,
}

impl Tool for JsonFlat {
    fn run(self) -> ExitCode {
        exit_code(flat(self.args))
    }
}

impl Tool for JsonNest {
    fn run(self) -> ExitCode {
        exit_code(nest(self.args))
    }
}

impl Tool for JsonRe {
    fn run(self) -> ExitCode {
        exit_code(filter(self))
    }
}

fn exit_code(result: Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        }
    }
}

impl FromStr for Delimiter {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        if text.is_empty() {
            bail!("delimiter is empty");
        }
        Ok(Delimiter(text.to_owned()))
    }
}

impl Delimiter {
    fn join(&self, parts: &[String]) -> Result<String> {
        if let Some(part) = parts.iter().find(|part| part.contains(&self.0)) {
            bail!("delimiter found in key {part:?}");
        }
        Ok(parts.join(&self.0))
    }

    fn split<'a>(&self, key: &'a str) -> Vec<&'a str> {
        key.split(self.0.as_str()).collect()
    }
}

impl TryFrom<&JsonRe> for Filter {
    type Error = Error;

    fn try_from(args: &JsonRe) -> Result<Self> {
        let compile = |pattern: &String| {
            RegexBuilder::new(pattern)
                .case_insensitive(args.ignore_case)
                .build()
                .with_context(|| format!("compiling {pattern:?}"))
        };
        Ok(Filter {
            key: args.key.as_ref().map(compile).transpose()?,
            value: args.value.as_ref().map(compile).transpose()?,
            polarity: if args.invert {
                Polarity::Inverted
            } else {
                Polarity::Matching
            },
        })
    }
}

impl Filter {
    fn keeps(&self, key: &str, value: &str) -> bool {
        self.passes(self.key.as_ref(), key) && self.passes(self.value.as_ref(), value)
    }

    fn passes(&self, pattern: Option<&Regex>, text: &str) -> bool {
        pattern.is_none_or(|pattern| match self.polarity {
            Polarity::Matching => pattern.is_match(text),
            Polarity::Inverted => !pattern.is_match(text),
        })
    }
}

fn flat(args: NestingArgs) -> Result<()> {
    let root = read(&args.data)?;
    let mut flat = Map::new();
    let mut stack = vec![(Vec::new(), root)];
    while let Some((prefix, object)) = stack.pop() {
        for (key, value) in object {
            let mut parts = prefix.clone();
            parts.push(key);
            match value {
                Value::Object(child) => stack.push((parts, child)),
                leaf => {
                    let flat_key = args.delimiter.join(&parts)?;
                    if flat.contains_key(&flat_key) {
                        bail!("duplicate key {flat_key:?}");
                    }
                    flat.insert(flat_key, leaf);
                }
            }
        }
    }
    write(flat)
}

fn nest(args: NestingArgs) -> Result<()> {
    let flat = read(&args.data)?;
    let mut nested = Map::new();
    for (flat_key, value) in flat {
        let mut parts = args.delimiter.split(&flat_key);
        let Some(key) = parts.pop().filter(|key| !key.is_empty()) else {
            continue;
        };
        parts.retain(|part| !part.is_empty());
        insert(&mut nested, &parts, key, value);
    }
    write(nested)
}

fn insert(object: &mut Map<String, Value>, parents: &[&str], key: &str, value: Value) {
    let Some((parent, rest)) = parents.split_first() else {
        object.insert(key.to_owned(), value);
        return;
    };
    match object
        .entry(*parent)
        .or_insert_with(|| Value::Object(Map::new()))
    {
        Value::Object(child) => insert(child, rest, key, value),
        leaf => {
            let mut child = Map::new();
            insert(&mut child, rest, key, value);
            *leaf = Value::Object(child);
        }
    }
}

fn filter(args: JsonRe) -> Result<()> {
    let filter = Filter::try_from(&args)?;
    let mut matched = Map::new();
    for (key, value) in read(&args.data)? {
        let Value::String(text) = &value else {
            bail!("value of {key:?} is not a string");
        };
        if filter.keeps(&key, text) {
            matched.insert(key, value);
        }
    }
    write(matched)
}

fn read(source: &Source) -> Result<Map<String, Value>> {
    let mut text = Vec::new();
    source
        .open()?
        .read_to_end(&mut text)
        .context("reading json")?;
    match serde_json::from_slice::<Value>(&text).context("parsing json")? {
        Value::Object(object) => Ok(object),
        _ => bail!("expected a json object"),
    }
}

fn write(object: Map<String, Value>) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&object)?);
    Ok(())
}
