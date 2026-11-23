//! A blocking client for icanhazip.com, which reports the caller's public address.

use std::net::{AddrParseError, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::time::Duration;

use reqwest::Url;
use reqwest::blocking::Client;

const IPV4_ENDPOINT: &str = "https://ipv4.icanhazip.com";
const IPV6_ENDPOINT: &str = "https://ipv6.icanhazip.com";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] bertbox_http::Error),
    #[error("{url} answered {body:?}, which is not an address")]
    Address {
        url: Box<Url>,
        body: String,
        #[source]
        source: AddrParseError,
    },
}

pub struct Api {
    client: Client,
    ipv4: Url,
    ipv6: Url,
}

impl Api {
    /// A client for the public icanhazip.com endpoints.
    pub fn new() -> Result<Self, Error> {
        let client = bertbox_http::client_builder()?
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(bertbox_http::Error::Client)?;
        Ok(Self {
            client,
            ipv4: Url::parse(IPV4_ENDPOINT).expect("the endpoint is a valid url"),
            ipv6: Url::parse(IPV6_ENDPOINT).expect("the endpoint is a valid url"),
        })
    }

    /// The public IPv4 address requests from this host arrive from.
    pub fn ipv4(&self) -> Result<Ipv4Addr, Error> {
        self.address(&self.ipv4)
    }

    /// The public IPv6 address requests from this host arrive from.
    pub fn ipv6(&self) -> Result<Ipv6Addr, Error> {
        self.address(&self.ipv6)
    }

    fn address<T: FromStr<Err = AddrParseError>>(&self, url: &Url) -> Result<T, Error> {
        let request = self.client.get(url.clone());
        let reply = bertbox_http::fetch(&self.client, request)?.into_success()?;
        reply.body.trim().parse().map_err(|source| Error::Address {
            url: Box::new(reply.url.clone()),
            body: reply.body.clone(),
            source,
        })
    }
}
