use std::path::PathBuf;

pub enum Color {
    Always,
    Never,
}

pub enum Headers {
    Omitted,
    Multiple,
    Full,
}

pub enum Caching {
    Pathcacher,
    Bypass,
}

pub struct Config {
    pub files: Vec<PathBuf>,
    pub color: Color,
    pub headers: Headers,
    pub caching: Caching,
}
