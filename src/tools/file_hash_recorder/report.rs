use std::collections::{BTreeMap, HashMap};
use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::tools::file_hash_recorder::config::{JsonStyle, Naming};
use crate::tools::file_hash_recorder::db::Scope;
use crate::tools::file_hash_recorder::record::{ContentKey, FileRecord};

const UNIT_STEP: u64 = 1024;

pub type Records = BTreeMap<String, FileRecord>;

pub fn print_json(value: &impl Serialize, style: JsonStyle) -> Result<()> {
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

pub fn dupes(
    records: Records,
    scope: &Scope,
    db_dir: &Path,
    naming: Naming,
) -> Result<Vec<Vec<PathBuf>>> {
    let mut group_of: HashMap<ContentKey, usize> = HashMap::new();
    let mut groups: Vec<Vec<PathBuf>> = Vec::new();
    for (name, record) in renamed(records, scope, db_dir, naming)? {
        let next = groups.len();
        let group = *group_of
            .entry(record.fingerprint.content_key())
            .or_insert(next);
        if group == next {
            groups.push(Vec::new());
        }
        groups[group].push(name);
    }
    Ok(groups.into_iter().filter(|names| names.len() > 1).collect())
}

pub fn renamed(
    records: Records,
    scope: &Scope,
    db_dir: &Path,
    naming: Naming,
) -> Result<BTreeMap<PathBuf, FileRecord>> {
    records
        .into_iter()
        .map(|(name, record)| -> Result<(PathBuf, FileRecord)> {
            let name = Path::new(&name);
            let shown = match (naming, scope) {
                (Naming::Absolute, _) => db_dir.join(name),
                (Naming::Relative, Scope::Under(dir)) => name
                    .strip_prefix(dir)
                    .with_context(|| format!("{} is not under {dir}", name.display()))?
                    .to_path_buf(),
                (Naming::Relative, _) => name.to_path_buf(),
            };
            Ok((shown, record))
        })
        .collect()
}

fn human_readable(size: u64) -> String {
    let Some(exponent @ 1..) = size.checked_ilog(UNIT_STEP) else {
        return size.to_string();
    };
    let unit = match exponent {
        1 => 'K',
        2 => 'M',
        3 => 'G',
        4 => 'T',
        5 => 'P',
        _ => 'E',
    };
    let scaled = size as f64 / (UNIT_STEP as f64).powi(exponent as i32);
    format!("{scaled:.1}{unit}")
}
