use std::path::PathBuf;

pub enum Action {
    Report,
    Unique,
    Remove { dry_run: bool },
    Move(PathBuf),
}

pub struct Config {
    pub roots: Vec<PathBuf>,
    pub action: Action,
}
