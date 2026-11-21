use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;

use crate::tools::archive::extract::config::Names;

// Fields are declared in the order they are printed.
#[derive(Default, Serialize)]
pub struct Entry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Default)]
pub struct Contents(BTreeMap<Vec<u8>, Entry>);

impl Contents {
    pub fn named(names: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self(
            names
                .into_iter()
                .map(|name| (name, Entry::default()))
                .collect(),
        )
    }

    pub fn insert(&mut self, name: Vec<u8>, entry: Entry) -> Option<Entry> {
        self.0.insert(name, entry)
    }

    pub fn contains(&self, name: &[u8]) -> bool {
        self.0.contains_key(name)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn encoded(self, names: &Names) -> BTreeMap<Vec<u8>, Entry> {
        match names {
            Names::Raw => self.0,
            Names::Base64 => self
                .0
                .into_iter()
                .map(|(name, entry)| (STANDARD.encode(name).into_bytes(), entry))
                .collect(),
        }
    }

    pub fn json(self, names: &Names) -> Result<String> {
        let object: BTreeMap<String, Entry> = self
            .encoded(names)
            .into_iter()
            .map(|(name, entry)| {
                let name = String::from_utf8(name).context("an entry name is not valid UTF-8")?;
                Ok((name, entry))
            })
            .collect::<Result<_>>()?;
        serde_json::to_string_pretty(&object).context("rendering the listing")
    }

    pub fn lines(self, names: &Names) -> Vec<u8> {
        let names: Vec<Vec<u8>> = self.encoded(names).into_keys().collect();
        let mut file_parents = BTreeSet::new();
        for name in names.iter().filter(|name| !name.ends_with(b"/")) {
            let parts: Vec<&[u8]> = trim_slashes(name).split(|byte| *byte == b'/').collect();
            for depth in 1..parts.len() {
                file_parents.insert(parts[..depth].join(&b'/'));
            }
        }

        let mut listed: Vec<&Vec<u8>> = names
            .iter()
            .filter(|name| !name.ends_with(b"/") || !file_parents.contains(trim_slashes(name)))
            .collect();
        listed.sort_by_key(|name| (name.to_ascii_lowercase(), *name));

        let mut lines = Vec::new();
        for name in listed {
            lines.extend_from_slice(name);
            lines.push(b'\n');
        }
        lines
    }
}

impl IntoIterator for Contents {
    type Item = (Vec<u8>, Entry);
    type IntoIter = std::collections::btree_map::IntoIter<Vec<u8>, Entry>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

fn trim_slashes(name: &[u8]) -> &[u8] {
    let start = name
        .iter()
        .position(|byte| *byte != b'/')
        .unwrap_or(name.len());
    let end = name
        .iter()
        .rposition(|byte| *byte != b'/')
        .map_or(start, |last| last + 1);
    &name[start..end]
}

pub fn split_fields(line: &[u8], max_splits: usize) -> Vec<&[u8]> {
    let mut fields = Vec::new();
    let mut rest = line.trim_ascii_start();
    while !rest.is_empty() {
        if fields.len() == max_splits {
            fields.push(rest);
            break;
        }
        let end = rest
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(rest.len());
        fields.push(&rest[..end]);
        rest = rest[end..].trim_ascii_start();
    }
    fields
}

pub fn lines(output: &[u8]) -> Vec<&[u8]> {
    if output.is_empty() {
        return Vec::new();
    }
    output
        .strip_suffix(b"\n")
        .unwrap_or(output)
        .split(|byte| *byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .collect()
}
