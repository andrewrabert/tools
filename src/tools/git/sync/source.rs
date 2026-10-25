use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, bail};
use winnow::Parser;
use winnow::combinator::{opt, preceded, separated_pair, terminated};
use winnow::error::{EmptyError, ParseError};
use winnow::token::{one_of, rest, take_till, take_while};

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
            let (host, path) = host_and_path
                .parse(repo.as_str())
                .map_err(|_: ParseError<&str, EmptyError>| anyhow!("invalid repo"))?;
            return Ok(Source {
                host: Host::named(host),
                path: path.to_owned(),
            });
        }

        let (scheme, netloc, path) = url
            .parse(repo.as_str())
            .map_err(|_: ParseError<&str, EmptyError>| anyhow!("invalid repo"))?;
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

fn host_and_path<'i>(input: &mut &'i str) -> winnow::Result<(&'i str, &'i str), EmptyError> {
    separated_pair(take_till(0.., '/'), '/', rest).parse_next(input)
}

fn url<'i>(input: &mut &'i str) -> winnow::Result<(&'i str, &'i str, &'i str), EmptyError> {
    let scheme = opt(scheme).parse_next(input)?.unwrap_or_default();
    let netloc = opt(netloc).parse_next(input)?.unwrap_or_default();
    let path = terminated(take_till(0.., ['?', '#']), rest).parse_next(input)?;
    Ok((scheme, netloc, path.trim_matches('/')))
}

fn scheme<'i>(input: &mut &'i str) -> winnow::Result<&'i str, EmptyError> {
    terminated(
        (
            one_of(|c: char| c.is_ascii_alphabetic()),
            take_while(0.., |c: char| {
                c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')
            }),
        )
            .take(),
        ':',
    )
    .parse_next(input)
}

fn netloc<'i>(input: &mut &'i str) -> winnow::Result<&'i str, EmptyError> {
    preceded("//", take_till(0.., ['/', '?', '#'])).parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(repo: &str) -> (String, String) {
        let source: Source = repo.parse().expect("valid repo");
        (source.host.to_string(), source.path)
    }

    fn error(repo: &str) -> String {
        let Err(error) = repo.parse::<Source>() else {
            panic!("{repo} is a valid repo");
        };
        error.to_string()
    }

    fn pair(host: &str, path: &str) -> (String, String) {
        (host.to_owned(), path.to_owned())
    }

    #[test]
    fn host_slash_path() {
        assert_eq!(
            parsed("example.com/owner/repo"),
            pair("example.com", "owner/repo")
        );
    }

    #[test]
    fn host_without_path_is_an_error() {
        assert_eq!(error("repo"), "invalid repo");
    }

    #[test]
    fn repo_is_lowercased() {
        assert_eq!(
            parsed("HTTPS://GitHub.com/Owner/Repo"),
            pair("github", "owner/repo")
        );
    }

    #[test]
    fn github_is_trimmed_to_owner_and_repo() {
        assert_eq!(
            parsed("https://github.com/owner/repo/tree/main"),
            pair("github", "owner/repo")
        );
        assert_eq!(
            parsed("https://www.github.com/owner/repo/issues/1"),
            pair("github", "owner/repo")
        );
    }

    #[test]
    fn github_owner_only_is_an_error() {
        assert_eq!(error("https://github.com/owner"), "invalid github url");
    }

    #[test]
    fn aur_package_is_its_name() {
        assert_eq!(
            parsed("https://aur.archlinux.org/packages/yay"),
            pair("aur", "yay")
        );
    }

    #[test]
    fn aur_packages_without_name_is_an_error() {
        assert_eq!(
            error("https://aur.archlinux.org/packages"),
            "invalid aur url"
        );
        assert_eq!(
            error("https://aur.archlinux.org/packages/"),
            "invalid aur url"
        );
    }

    #[test]
    fn query_and_fragment_are_dropped() {
        assert_eq!(
            parsed("https://codeberg.org/owner/repo?tab=readme#top"),
            pair("codeberg", "owner/repo")
        );
        assert_eq!(
            parsed("https://gitlab.com/group/repo/#readme"),
            pair("gitlab", "group/repo")
        );
    }

    #[test]
    fn netloc_ends_at_query() {
        assert_eq!(
            error("https://example.com?path=/owner/repo"),
            "invalid example.com url"
        );
    }

    #[test]
    fn unknown_netloc_is_its_host() {
        assert_eq!(
            parsed("https://git.example.com/owner/repo/"),
            pair("git.example.com", "owner/repo")
        );
    }

    #[test]
    fn unknown_netloc_without_owner_is_an_error() {
        assert_eq!(
            error("https://git.example.com/repo"),
            "invalid git.example.com url"
        );
    }

    #[test]
    fn scp_like_scheme_is_its_host() {
        assert_eq!(parsed("github:owner/repo"), pair("github", "owner/repo"));
    }

    #[test]
    fn invalid_scheme_is_a_missing_host() {
        assert_eq!(error("1abc:x"), "missing git host");
        assert_eq!(error("git@github.com:owner/repo"), "missing git host");
    }
}
