use std::borrow::Cow;
use std::collections::HashSet;
use std::convert::Infallible;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use anyhow::{Context, Result};
use linkify::{LinkFinder, LinkKind};

/// A URL scheme, lowercase (RFC 3986 section 3.1).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Scheme(Cow<'static, str>);

impl Scheme {
    pub const FTP: Self = Self(Cow::Borrowed("ftp"));
    pub const HTTP: Self = Self(Cow::Borrowed("http"));
    pub const HTTPS: Self = Self(Cow::Borrowed("https"));

    pub fn new(scheme: &str) -> Self {
        Self(Cow::Owned(scheme.to_ascii_lowercase()))
    }
}

impl Deref for Scheme {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Scheme {
    type Err = Infallible;

    fn from_str(scheme: &str) -> Result<Self, Infallible> {
        Ok(Self::new(scheme))
    }
}

pub struct Url<'t> {
    scheme: Scheme,
    text: &'t str,
}

impl<'t> Url<'t> {
    pub fn new(text: &'t str) -> Result<Self> {
        let (scheme, _) = text
            .split_once("://")
            .with_context(|| format!("{text:?} has no scheme"))?;
        Ok(Self {
            scheme: Scheme::new(scheme),
            text,
        })
    }

    pub fn scheme(&self) -> &Scheme {
        &self.scheme
    }
}

impl Deref for Url<'_> {
    type Target = str;

    fn deref(&self) -> &str {
        self.text
    }
}

/// URLs in `text`, in order of first appearance, each once.
pub fn unique_urls(text: &str) -> impl Iterator<Item = Result<Url<'_>>> {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]);
    let mut seen = HashSet::new();
    finder
        .links(text)
        .filter(move |link| seen.insert(link.as_str()))
        .map(|link| Url::new(link.as_str()))
}
