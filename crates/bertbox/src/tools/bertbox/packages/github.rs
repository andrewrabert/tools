use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GitHubRepository {
    pub owner: &'static str,
    pub name: &'static str,
}

impl fmt::Display for GitHubRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}
