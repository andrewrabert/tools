use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, bail};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    Aur,
    Codeberg,
    Github,
    Gitlab,
    KdeInvent,
    Nullsum,
}

impl Forge {
    pub const ALL: [Forge; 6] = [
        Forge::Aur,
        Forge::Codeberg,
        Forge::Github,
        Forge::Gitlab,
        Forge::KdeInvent,
        Forge::Nullsum,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Forge::Aur => "aur",
            Forge::Codeberg => "codeberg",
            Forge::Github => "github",
            Forge::Gitlab => "gitlab",
            Forge::KdeInvent => "invent.kde.org",
            Forge::Nullsum => "nullsum",
        }
    }

    pub fn from_name(name: &str) -> Option<Forge> {
        Forge::ALL.into_iter().find(|forge| forge.name() == name)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum Host {
    Forge(Forge),
    Other(String),
}

impl Host {
    fn named(name: &str) -> Host {
        Forge::from_name(name).map_or_else(|| Host::Other(name.to_owned()), Host::Forge)
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Host::Forge(forge) => f.write_str(forge.name()),
            Host::Other(name) => f.write_str(name),
        }
    }
}

pub struct Source {
    pub host: Host,
    pub path: String,
}

impl Source {
    pub fn dir(&self, root: &Path) -> PathBuf {
        root.join(self.host.to_string()).join(&self.path)
    }

    pub fn clone_url(&self) -> String {
        match self.host {
            Host::Forge(Forge::KdeInvent) => format!("https://{}/{}", self.host, self.path),
            _ => self.to_string(),
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.path)
    }
}

impl FromStr for Source {
    type Err = anyhow::Error;

    fn from_str(repo: &str) -> anyhow::Result<Self> {
        let repo = repo.to_lowercase();
        if !repo.contains(':') {
            let (host, path) = repo.split_once('/').context("invalid repo")?;
            return Ok(Source {
                host: Host::named(host),
                path: path.to_owned(),
            });
        }

        let (scheme, rest) = split_scheme(&repo);
        let (netloc, path) = match rest.strip_prefix("//") {
            Some(rest) => rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len())),
            None => ("", rest),
        };
        let path = path
            .split(['?', '#'])
            .next()
            .unwrap_or_default()
            .trim_matches('/');
        let segments: Vec<&str> = path.split('/').collect();
        let owned = |name: &str| {
            if segments.len() < 2 {
                bail!("invalid {name} url");
            }
            Ok(path.to_owned())
        };

        let (host, path) = match scheme {
            "http" | "https" => match netloc {
                "codeberg.org" => (Host::Forge(Forge::Codeberg), owned("codeberg")?),
                "github.com" | "www.github.com" => {
                    owned("github")?;
                    (Host::Forge(Forge::Github), segments[..2].join("/"))
                }
                "gitlab.com" | "www.gitlab.com" => (Host::Forge(Forge::Gitlab), owned("gitlab")?),
                "aur.archlinux.org" => {
                    let path = match segments.as_slice() {
                        ["packages", name, ..] => name.to_string(),
                        ["packages"] => bail!("invalid aur url"),
                        _ => path.to_owned(),
                    };
                    (Host::Forge(Forge::Aur), path)
                }
                "git.nullsum.net" => (Host::Forge(Forge::Nullsum), owned("git.nullsum.net")?),
                "invent.kde.org" => (Host::Forge(Forge::KdeInvent), owned("invent.kde.org")?),
                netloc => (Host::named(netloc), owned(netloc)?),
            },
            "" => bail!("missing git host"),
            scheme => (Host::named(scheme), path.to_owned()),
        };
        Ok(Source { host, path })
    }
}

fn split_scheme(url: &str) -> (&str, &str) {
    let Some((scheme, rest)) = url.split_once(':') else {
        return ("", url);
    };
    let valid = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if valid { (scheme, rest) } else { ("", url) }
}
