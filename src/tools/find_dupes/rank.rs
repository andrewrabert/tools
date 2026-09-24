use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

static LF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.+)(\.~\d+~)+$").unwrap());
static FIREFOX_PAREN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(.)(\(\d+\))(\.[^.]+|\.tar\.[^.]+)$").unwrap());
static FIREFOX_DASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(.)(-\d+)(\.[^.]+|\.tar\.[^.]+)$").unwrap());
static OBSIDIAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(.)\s+(\d+)(\.[^.]+|\.tar\.[^.]+)$").unwrap());

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct RetainRank {
    dupe_score: u8,
    name_is_lf: bool,
    any_component_is_lf: bool,
    depth: usize,
    path: String,
}

impl RetainRank {
    pub fn of(path: &Path) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (stripped, is_lf) = strip_lf(&name);
        let is_firefox = FIREFOX_PAREN.is_match(&stripped) || FIREFOX_DASH.is_match(&stripped);
        let is_obsidian = OBSIDIAN.is_match(&name);
        let lf_components: Vec<bool> = path
            .components()
            .map(|c| LF.is_match(&c.as_os_str().to_string_lossy()))
            .collect();
        Self {
            dupe_score: u8::from(is_lf) + u8::from(is_firefox) + u8::from(is_obsidian),
            name_is_lf: lf_components.last().copied().unwrap_or(false),
            any_component_is_lf: lf_components.iter().any(|b| *b),
            depth: lf_components.len(),
            path: path.to_string_lossy().into_owned(),
        }
    }
}

fn strip_lf(name: &str) -> (String, bool) {
    let mut name = name.to_owned();
    let mut stripped = false;
    while let Some(caps) = LF.captures(&name) {
        name = caps[1].to_owned();
        stripped = true;
    }
    (name, stripped)
}
