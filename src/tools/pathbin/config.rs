use std::path::PathBuf;

pub enum Grouping {
    Flat,
    ByCommand,
    ByDirectory,
}

pub struct Config {
    pub dirs: Vec<PathBuf>,
    pub grouping: Grouping,
}
