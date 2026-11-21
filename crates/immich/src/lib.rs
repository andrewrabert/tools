//! A blocking client for the Immich asset and library API.

use std::collections::HashSet;
use std::num::ParseIntError;

use bertbox_http::{BaseUrl, json_body, sensitive_header};
use reqwest::Url;
use reqwest::blocking::{Client, ClientBuilder, RequestBuilder};
use reqwest::header::{HeaderMap, HeaderName};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;

const PAGE_SIZE: u32 = 1000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] bertbox_http::Error),
    #[error("parsing the next page {page:?}")]
    NextPage {
        page: String,
        #[source]
        source: ParseIntError,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub original_path: String,
    pub is_offline: bool,
}

#[derive(Deserialize)]
pub struct Library {
    pub id: String,
}

#[derive(Deserialize)]
struct Search {
    assets: SearchPage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchPage {
    items: Vec<Asset>,
    next_page: Option<String>,
}

pub struct Api {
    client: Client,
    base: BaseUrl,
}

impl Api {
    /// Builds a client from `builder` that authenticates to the server at `base` with `api_key`.
    pub fn new(builder: ClientBuilder, base: &str, api_key: &str) -> Result<Self, Error> {
        let base = BaseUrl::parse(base, &["api"])?;
        let key = sensitive_header("the api key", api_key)?;
        let client = builder
            .default_headers(HeaderMap::from_iter([(
                HeaderName::from_static("x-api-key"),
                key,
            )]))
            .build()
            .map_err(bertbox_http::Error::Client)?;
        Ok(Self { client, base })
    }

    /// Lists every trashed asset once, in the order the server returns them.
    pub fn trash_assets(&self) -> Result<Vec<Asset>, Error> {
        let mut seen = HashSet::new();
        let mut assets = Vec::new();
        let mut page = Some(1u32);
        while let Some(number) = page {
            let body = json!({
                "trashedAfter": "2000-01-01T00:00:00.000Z",
                "size": PAGE_SIZE,
                "page": number,
            });
            let request = json_body(self.client.post(self.url(&["search", "metadata"])), &body)?;
            let search: Search = self.json(request)?;
            assets.extend(
                search
                    .assets
                    .items
                    .into_iter()
                    .filter(|asset| seen.insert(asset.id.clone())),
            );
            page = search
                .assets
                .next_page
                .map(|next| {
                    next.parse()
                        .map_err(|source| Error::NextPage { page: next, source })
                })
                .transpose()?;
        }
        Ok(assets)
    }

    pub fn delete_assets(&self, ids: &[&str]) -> Result<(), Error> {
        let body = json!({ "force": true, "ids": ids });
        self.send(json_body(self.client.delete(self.url(&["assets"])), &body)?)
    }

    pub fn restore_assets(&self, ids: &[&str]) -> Result<(), Error> {
        let body = json!({ "ids": ids });
        let url = self.url(&["trash", "restore", "assets"]);
        self.send(json_body(self.client.post(url), &body)?)
    }

    pub fn libraries(&self) -> Result<Vec<Library>, Error> {
        self.json(self.client.get(self.url(&["libraries"])))
    }

    pub fn scan_library(&self, id: &str) -> Result<(), Error> {
        self.send(self.client.post(self.url(&["libraries", id, "scan"])))
    }

    fn url(&self, segments: &[&str]) -> Url {
        self.base.join(segments.iter().copied())
    }

    /// Sends a request and returns its target and body, failing on a non-success status.
    fn fetch(&self, request: RequestBuilder) -> Result<(String, String), Error> {
        let reply = bertbox_http::fetch(&self.client, request)?;
        let target = reply.target();
        Ok((target, reply.into_success()?))
    }

    fn send(&self, request: RequestBuilder) -> Result<(), Error> {
        self.fetch(request).map(drop)
    }

    fn json<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T, Error> {
        let (target, body) = self.fetch(request)?;
        Ok(bertbox_http::decode(&target, &body)?)
    }
}
