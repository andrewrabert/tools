use std::fmt;
use std::path::Path;

use crate::tools::find_file_exts::config::Folding;

const WHOLE_NAME_SUFFIXES: [&str; 1] = [".DS_Store"];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Suffix(String);

impl Suffix {
    pub fn of(path: &Path, folding: &Folding) -> Self {
        let name = path.file_name().map(|name| name.to_string_lossy());
        let suffix = match name {
            Some(name) if WHOLE_NAME_SUFFIXES.contains(&name.as_ref()) => name.into_owned(),
            _ => match path.extension() {
                Some(extension) if !extension.is_empty() => {
                    format!(".{}", extension.to_string_lossy())
                }
                _ => String::new(),
            },
        };
        match folding {
            Folding::Preserve => Suffix(suffix),
            Folding::Lowercase => Suffix(suffix.to_lowercase()),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Suffix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            f.write_str("\"\"")
        } else {
            f.write_str(&self.0)
        }
    }
}
