//! A blocking client for the releases GitHub serves at github.com.

use std::fmt;

use bertbox_http::{BaseUrl, Download};
use reqwest::Url;
use reqwest::blocking::Client;

const ENDPOINT: &str = "https://github.com";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] bertbox_http::Error),
    /// The repository answers under another owner or name.
    #[error("{url} now redirects to {location}")]
    Moved { url: Box<Url>, location: Box<Url> },
    /// The repository has no published release.
    #[error("{url} redirects to {location}")]
    NoRelease { url: Box<Url>, location: Box<Url> },
    #[error("no checksum published for {asset}")]
    NoChecksum { asset: String },
}

/// An `owner/name` pair naming one repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Repository<'a> {
    pub owner: &'a str,
    pub name: &'a str,
}

impl fmt::Display for Repository<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

pub struct Api {
    client: Client,
    base: BaseUrl,
}

impl Api {
    /// A client for github.com that follows redirects, as release downloads need.
    pub fn new() -> Result<Self, Error> {
        let endpoint = Url::parse(ENDPOINT).expect("the endpoint is a valid url");
        let base = BaseUrl::new(endpoint, &[])?;
        let client = bertbox_http::client_builder()?
            .build()
            .map_err(bertbox_http::Error::Client)?;
        Ok(Self { client, base })
    }

    /// The tag of the repository's latest release.
    pub fn latest_tag(&self, repository: Repository<'_>) -> Result<String, Error> {
        let url = self.releases(repository, &["latest"]);
        let request = self.client.head(url.clone());
        let location = bertbox_http::fetch(&self.client, request)?
            .into_success()?
            .url;
        let segments: Vec<&str> = match location.path_segments() {
            Some(segments) if location.host_str() == self.base.url().host_str() => {
                segments.collect()
            }
            _ => {
                return Err(Error::NoRelease {
                    url: Box::new(url),
                    location: Box::new(location),
                });
            }
        };
        if segments.get(..2) != Some(&[repository.owner, repository.name][..]) {
            return Err(Error::Moved {
                url: Box::new(url),
                location: Box::new(location),
            });
        }
        // The latest release redirects to `<owner>/<repo>/releases/tag/<tag>`.
        let tag = match segments[2..] {
            ["releases", "tag", ref tag @ ..] if !tag.is_empty() && !tag.contains(&"") => {
                tag.join("/")
            }
            _ => {
                return Err(Error::NoRelease {
                    url: Box::new(url),
                    location: Box::new(location),
                });
            }
        };
        Ok(tag)
    }

    /// Starts downloading one asset of a release.
    pub fn asset(
        &self,
        repository: Repository<'_>,
        tag: &str,
        asset: &str,
    ) -> Result<Download, Error> {
        let url = self.releases(repository, &["download", tag, asset]);
        Ok(bertbox_http::download(&self.client, url)?)
    }

    /// The SHA-256 digest of `asset`, as listed in the release's `checksums` asset.
    pub fn sha256(
        &self,
        repository: Repository<'_>,
        tag: &str,
        checksums: &str,
        asset: &str,
    ) -> Result<String, Error> {
        let url = self.releases(repository, &["download", tag, checksums]);
        let reply = bertbox_http::fetch(&self.client, self.client.get(url))?.into_success()?;
        reply
            .body
            .lines()
            .filter_map(checksum_line)
            .find(|(_, name)| *name == asset)
            .map(|(digest, _)| digest.to_owned())
            .ok_or_else(|| Error::NoChecksum {
                asset: asset.to_owned(),
            })
    }

    fn releases(&self, repository: Repository<'_>, segments: &[&str]) -> Url {
        let path = [repository.owner, repository.name, "releases"];
        self.base
            .join(path.into_iter().chain(segments.iter().copied()))
    }
}

/// The digest and file name of one `sha256sum` output line.
fn checksum_line(line: &str) -> Option<(&str, &str)> {
    let (digest, rest) = line.split_once(' ')?;
    let name = rest.strip_prefix(' ').or_else(|| rest.strip_prefix('*'))?;
    Some((digest, name))
}
