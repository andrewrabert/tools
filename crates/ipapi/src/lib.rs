//! A blocking client for the ip-api.com geolocation API.

use bertbox_http::BaseUrl;
use reqwest::Url;
use reqwest::blocking::Client;
use serde_json::Value;

const ENDPOINT: &str = "http://ip-api.com/json";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] bertbox_http::Error),
}

pub struct Api {
    client: Client,
    base: BaseUrl,
}

impl Api {
    /// A client for the public ip-api.com endpoint.
    pub fn new() -> Result<Self, Error> {
        let endpoint = Url::parse(ENDPOINT).expect("the endpoint is a valid url");
        let base = BaseUrl::new(endpoint, &[])?;
        let client = bertbox_http::client_builder()?
            .build()
            .map_err(bertbox_http::Error::Client)?;
        Ok(Self { client, base })
    }

    /// The geolocation of an IP address or domain, as the service reports it.
    pub fn lookup(&self, address: &str) -> Result<Value, Error> {
        let request = self.client.get(self.base.join([address]));
        let reply = bertbox_http::fetch(&self.client, request)?.into_success()?;
        Ok(reply.json()?)
    }
}
