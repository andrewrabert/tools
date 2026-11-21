use std::num::NonZeroUsize;
use std::path::PathBuf;

use crate::tools::archive::create::format::Format;
use crate::tools::archive::password::Password;

#[derive(Clone, Copy)]
pub enum Speed {
    Fast,
    Best,
}

pub enum Placement {
    BesideSource,
    Under(PathBuf),
    Exact(PathBuf),
}

pub enum SourceFate {
    Keep,
    Remove,
}

pub struct Config {
    pub sources: Vec<PathBuf>,
    pub format: Format,
    pub speed: Speed,
    pub placement: Placement,
    pub password: Option<Password>,
    pub source_fate: SourceFate,
    pub jobs: NonZeroUsize,
}
