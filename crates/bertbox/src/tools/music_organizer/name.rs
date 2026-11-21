use std::fs;
use std::path::{Path, PathBuf};

use crate::tools::music_organizer::log::Log;

const IGNORED_PATH_CHARS: [char; 11] = ['&', '/', '\\', '.', ':', '*', '?', '"', '<', '>', '|'];

/// Python's `str.isprintable`, near enough: control and non-space whitespace fail.
pub fn is_printable(c: char) -> bool {
    !(c.is_control() || (c.is_whitespace() && c != ' '))
}

/// A folder name for a tag value, or `None` when it holds an unprintable character.
pub fn folder_name(tag_value: &str) -> Option<String> {
    let mut chars = String::new();
    for c in tag_value.chars() {
        if chars.is_empty() && c == '.' {
            continue;
        }
        if IGNORED_PATH_CHARS.contains(&c) {
            continue;
        }
        if !is_printable(c) {
            return None;
        }
        chars.push(c);
    }
    Some(collapse_whitespace(&chars))
}

/// A file name for a track title.
pub fn title_name(title: &str) -> String {
    let replaced: String = title
        .chars()
        .map(|c| {
            if IGNORED_PATH_CHARS.contains(&c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    collapse_whitespace(&replaced)
}

pub fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Rename `source` to `name` beside it, unless that name is taken; returns where the file now is.
pub fn safe_rename(log: &mut Log, source: &Path, name: &str, dry_run: bool) -> PathBuf {
    let target = source.with_file_name(name);
    if source == target {
        return target;
    }
    if target.exists() {
        log.error(format!(
            "Error renaming \"{}\" to \"{}\": already exists",
            source.display(),
            target.display()
        ));
        return source.to_path_buf();
    }
    log.info(format!(
        "Renaming \"{}\" to \"{}\"",
        source.display(),
        target.display()
    ));
    if dry_run {
        return source.to_path_buf();
    }
    match fs::rename(source, &target) {
        Ok(()) => target,
        Err(error) => {
            log.error(format!(
                "Error renaming \"{}\" to \"{}\": {error}",
                source.display(),
                target.display()
            ));
            source.to_path_buf()
        }
    }
}
