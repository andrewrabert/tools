use std::num::NonZeroUsize;
use std::path::PathBuf;

pub enum Names {
    Raw,
    Base64,
}

pub enum Layout {
    Lines,
    Json,
}

pub enum Placement {
    Merged,
    ChildDirectory,
}

pub enum Target {
    Stdout,
    Directory {
        parent: Option<PathBuf>,
        placement: Placement,
    },
}

#[derive(Clone, Copy)]
pub enum Overwrite {
    Keep,
    Replace,
}

pub enum ArchiveFate {
    Keep,
    Remove,
}

pub enum Progress {
    Announced,
    Silent,
}

pub enum OnError {
    Fail,
    Continue,
}

pub enum Credentials {
    Absent,
    Prompt,
}

pub enum Diagnostics {
    Errors,
    Debug,
}

pub struct Extraction {
    pub target: Target,
    pub members: Vec<String>,
    pub overwrite: Overwrite,
    pub archive_fate: ArchiveFate,
    pub progress: Progress,
    pub jobs: NonZeroUsize,
}

pub enum Action {
    List { names: Names, layout: Layout },
    Volumes,
    Extract(Extraction),
}

pub struct Config {
    pub archives: Vec<PathBuf>,
    pub action: Action,
    pub credentials: Credentials,
    pub on_error: OnError,
    pub diagnostics: Diagnostics,
}
