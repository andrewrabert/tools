use std::collections::HashSet;
use std::fmt;
use std::io::{self, Write};
use std::str::FromStr;

use anyhow::{Error, bail};
use crossterm::style::Stylize;
use serde::{Deserialize, Serialize};

/// An `owner/repo` pair naming one repository.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RepoName {
    pub owner: String,
    pub name: String,
}

impl FromStr for RepoName {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((owner, name)) = value.split_once('/') else {
            bail!("expected owner/repo, got {value:?}");
        };
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            bail!("invalid repo format: {value}");
        }
        Ok(Self {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }
}

impl fmt::Display for RepoName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// Drops repeated names, keeping the first occurrence of each in order.
pub fn unique(repos: Vec<RepoName>) -> Vec<RepoName> {
    let mut seen = HashSet::new();
    repos
        .into_iter()
        .filter(|repo| seen.insert(repo.clone()))
        .collect()
}

/// A repository as the API returns it.
#[derive(Deserialize)]
pub struct ApiRepo {
    pub full_name: String,
    pub private: bool,
    pub archived: bool,
    pub description: Option<String>,
    pub html_url: String,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Private,
    Public,
}

impl Visibility {
    pub fn of(private: bool) -> Self {
        if private { Self::Private } else { Self::Public }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Public => "public",
        }
    }
}

/// A repository as the tool reports it.
#[derive(Serialize)]
pub struct Repo {
    pub name: String,
    pub visibility: Visibility,
    pub archived: bool,
    pub description: String,
    pub url: String,
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
            ("url", self.url.clone()),
        ];
        let width = fields.iter().map(|(key, _)| key.len()).max().unwrap_or(0);
        for (key, value) in fields {
            writeln!(out, "{} {value}", format!("{key:<width$}:").dim())?;
        }
        Ok(())
    }
}
