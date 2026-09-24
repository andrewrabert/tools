use std::collections::{BTreeMap, HashMap};
use std::env;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::tools::file_hash_recorder::config::{JsonStyle, Naming};
use crate::tools::file_hash_recorder::db::Scope;
use crate::tools::file_hash_recorder::record::{ContentKey, FileRecord};

const UNITS: [&str; 6] = ["", "K", "M", "G", "T", "P"];
const UNIT_STEP: f64 = 1024.0;

pub type Records = BTreeMap<String, FileRecord>;

pub fn print_json(value: &Value, style: JsonStyle) -> Result<()> {
    let text = match style {
        JsonStyle::Pretty => serde_json::to_string_pretty(value)?,
        JsonStyle::Compact => serde_json::to_string(value)?,
    };
    println!("{text}");
    Ok(())
}

pub fn size(records: &Records, root: &Path, naming: Naming) -> Result<()> {
    let total: u64 = records
        .values()
        .map(|record| record.fingerprint.stamp.size)
        .sum();
    let shown = match naming {
        Naming::Absolute => root,
        Naming::Relative => {
            let cwd = env::current_dir().context("resolving the current directory")?;
            match root.strip_prefix(cwd) {
                Ok(relative) if relative.as_os_str().is_empty() => Path::new("."),
                Ok(relative) => relative,
                Err(_) => root,
            }
        }
    };
    println!("{total}\t{}\t{}", human_readable(total), shown.display());
    Ok(())
}

pub fn list(records: Records, scope: &Scope, db_dir: &Path, naming: Naming) -> Value {
    let object: Map<String, Value> = renamed(records, scope, db_dir, naming)
        .iter()
        .map(|(name, record)| (name.clone(), record.to_json()))
        .collect();
    Value::Object(object)
}

pub fn dupes(records: Records, scope: &Scope, db_dir: &Path, naming: Naming) -> Value {
    let mut group_of: HashMap<ContentKey, usize> = HashMap::new();
    let mut groups: Vec<Vec<String>> = Vec::new();
    for (name, record) in renamed(records, scope, db_dir, naming) {
        let next = groups.len();
        let group = *group_of
            .entry(record.fingerprint.content_key())
            .or_insert(next);
        if group == next {
            groups.push(Vec::new());
        }
        groups[group].push(name);
    }
    groups
        .into_iter()
        .filter(|names| names.len() > 1)
        .map(Value::from)
        .collect()
}

fn renamed(records: Records, scope: &Scope, db_dir: &Path, naming: Naming) -> Records {
    records
        .into_iter()
        .map(|(name, record)| {
            let name = match (naming, scope) {
                (Naming::Absolute, _) => format!("{}/{name}", db_dir.display()),
                (Naming::Relative, Scope::Under(prefix)) => name
                    .strip_prefix(prefix.as_str())
                    .map_or(name.clone(), str::to_owned),
                (Naming::Relative, _) => name,
            };
            (name, record)
        })
        .collect()
}

fn human_readable(size: u64) -> String {
    let mut scaled = size as f64;
    for unit in UNITS {
        if scaled < UNIT_STEP {
            return if unit.is_empty() {
                size.to_string()
            } else {
                format!("{scaled:.1}{unit}")
            };
        }
        scaled /= UNIT_STEP;
    }
    format!("{scaled:.1}E")
}
