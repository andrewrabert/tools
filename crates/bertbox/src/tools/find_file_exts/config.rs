use std::path::PathBuf;

pub enum Folding {
    Preserve,
    Lowercase,
}

pub enum Output {
    Suffixes,
    Counts,
    Json,
}

pub struct Config {
    pub roots: Vec<PathBuf>,
    pub folding: Folding,
    pub output: Output,
}
