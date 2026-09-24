use std::num::NonZeroUsize;
use std::path::PathBuf;

use crate::tools::file_hash_recorder::archive::Extraction;
use crate::tools::file_hash_recorder::locate::Missing;

#[derive(Clone, Copy)]
pub enum JsonStyle {
    Pretty,
    Compact,
}

#[derive(Clone, Copy)]
pub enum Naming {
    Relative,
    Absolute,
}

#[derive(Clone, Copy)]
pub enum Feedback {
    ProgressBar,
    EachFile,
    Silent,
}

#[derive(Clone, Copy)]
pub enum ArchivePolicy {
    Ignore,
    Record(Extraction),
}

#[derive(Clone, Copy)]
pub enum OnError {
    Abort,
    Skip,
}

#[derive(Clone, Copy)]
pub enum Rehash {
    Changed,
    Everything,
}

pub struct UpdateOptions {
    pub rehash: Rehash,
    pub archives: ArchivePolicy,
    pub jobs: NonZeroUsize,
    pub on_error: OnError,
}

pub enum Action {
    Update(UpdateOptions),
    List,
    Dupes,
    Size,
    Verify,
}

pub enum Mode {
    Multihash,
    Recorded { database: Database, action: Action },
}

pub enum Database {
    Given(PathBuf),
    Discovered(Missing),
}

pub struct Config {
    pub roots: Vec<PathBuf>,
    pub mode: Mode,
    pub naming: Naming,
    pub feedback: Feedback,
    pub json: JsonStyle,
}
