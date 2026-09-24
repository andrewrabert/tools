use std::fmt;

use anyhow::{Context, Result};
use reqwest::Url;

const SCHEMES: [&str; 3] = ["ftp://", "http://", "https://"];
const IMPLIED_SCHEME: &str = "http://";
const YOUTUBE: &str = "youtube.com";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target(Url);

pub enum Site {
    YouTube,
    Http,
    Other,
}

pub enum Scheme {
    Required,
    Implied,
}

impl Target {
    fn parse(raw: &str, scheme: &Scheme) -> Result<Option<Self>> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Ok(None);
        }
        let lowered = raw.to_lowercase();
        let has_scheme = SCHEMES.iter().any(|scheme| lowered.starts_with(scheme));
        let url = match scheme {
            _ if has_scheme => raw.to_owned(),
            Scheme::Required => return Ok(None),
            Scheme::Implied => format!("{IMPLIED_SCHEME}{raw}"),
        };
        let url = Url::parse(&url).with_context(|| format!("parsing url {url}"))?;
        Ok(Some(Target(url)))
    }

    pub fn url(&self) -> &Url {
        &self.0
    }

    pub fn site(&self) -> Site {
        let on_youtube = self.0.host_str().is_some_and(|host| {
            host == YOUTUBE
                || host
                    .strip_suffix(YOUTUBE)
                    .is_some_and(|subdomain| subdomain.ends_with('.'))
        });
        match self.0.scheme() {
            _ if on_youtube => Site::YouTube,
            "http" | "https" => Site::Http,
            _ => Site::Other,
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Default)]
pub struct Targets(Vec<Target>);

impl Targets {
    pub fn extend<S: AsRef<str>>(
        &mut self,
        raw: impl IntoIterator<Item = S>,
        scheme: &Scheme,
    ) -> Result<()> {
        for raw in raw {
            if let Some(target) = Target::parse(raw.as_ref(), scheme)?
                && !self.0.contains(&target)
            {
                self.0.push(target);
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_slice(&self) -> &[Target] {
        &self.0
    }
}
