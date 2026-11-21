use std::path::PathBuf;

pub enum Mode {
    List,
    Check,
    Fix { force: bool },
}

pub struct Config {
    pub roots: Vec<PathBuf>,
    pub mode: Mode,
}
