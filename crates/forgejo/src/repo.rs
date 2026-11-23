use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use url::Url;

/// An `owner/repo` pair naming one repository.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RepoName {
    pub owner: String,
    pub name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseRepoNameError {
    #[error("expected owner/repo, got {0:?}")]
    MissingSlash(String),
    #[error("invalid repo format: {0}")]
    Invalid(String),
}

impl FromStr for RepoName {
    type Err = ParseRepoNameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((owner, name)) = value.split_once('/') else {
            return Err(ParseRepoNameError::MissingSlash(value.to_owned()));
        };
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            return Err(ParseRepoNameError::Invalid(value.to_owned()));
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

/// A repository as the API returns it.
#[derive(Deserialize)]
pub struct ApiRepo {
    pub full_name: String,
    pub private: bool,
    pub archived: bool,
    pub description: Option<String>,
    pub html_url: Url,
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
