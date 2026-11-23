use std::collections::HashSet;
use std::io::{self, Write};

use bertbox_forgejo::{ApiRepo, RepoName, Visibility};
use crossterm::style::Stylize;
use serde::Serialize;
use url::Url;

/// Drops repeated names, keeping the first occurrence of each in order.
pub fn unique(repos: Vec<RepoName>) -> Vec<RepoName> {
    let mut seen = HashSet::new();
    repos
        .into_iter()
        .filter(|repo| seen.insert(repo.clone()))
        .collect()
}

/// A repository as the tool reports it.
#[derive(Serialize)]
pub struct Repo {
    pub name: String,
    pub visibility: Visibility,
    pub archived: bool,
    pub description: String,
    pub url: Url,
}

impl From<ApiRepo> for Repo {
    fn from(repo: ApiRepo) -> Self {
        Self {
            name: repo.full_name,
            visibility: Visibility::of(repo.private),
            archived: repo.archived,
            description: repo.description.unwrap_or_default(),
            url: repo.html_url,
        }
    }
}

impl Repo {
    /// Writes one dimmed `key: value` line per field.
    pub fn write_fields(&self, out: &mut impl Write) -> io::Result<()> {
        let fields = [
            ("name", self.name.clone()),
            ("visibility", self.visibility.as_str().to_owned()),
            ("archived", self.archived.to_string()),
            ("description", self.description.clone()),
            ("url", self.url.to_string()),
        ];
        let width = fields.iter().map(|(key, _)| key.len()).max().unwrap_or(0);
        for (key, value) in fields {
            writeln!(out, "{} {value}", format!("{key:<width$}:").dim())?;
        }
        Ok(())
    }
}
