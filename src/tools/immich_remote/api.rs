use std::collections::HashSet;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::Url;
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::http;

const PAGE_SIZE: u32 = 1000;

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
    base: Url,
}

impl Api {
    pub fn new(base: &str, api_key: &str) -> Result<Self> {
        let mut base = Url::parse(base).with_context(|| format!("parsing the url {base:?}"))?;
        base.path_segments_mut()
            .map_err(|()| anyhow!("the immich url cannot hold a path"))?
            .pop_if_empty()
            .push("api");
        let mut key =
            HeaderValue::from_str(api_key).context("the api key is not a valid header value")?;
        key.set_sensitive(true);
        let client = http::client_builder()?
            .default_headers(HeaderMap::from_iter([(
                HeaderName::from_static("x-api-key"),
                key,
            )]))
            .build()
            .context("building the http client")?;
        Ok(Self { client, base })
    }

    /// Lists every trashed asset once, in the order the server returns them.
    pub fn trash_assets(&self) -> Result<Vec<Asset>> {
        let mut seen = HashSet::new();
        let mut assets = Vec::new();
        let mut page = Some(1u32);
        while let Some(number) = page {
            let body = json!({
                "trashedAfter": "2000-01-01T00:00:00.000Z",
                "size": PAGE_SIZE,
                "page": number,
            });
            let request = with_body(self.client.post(self.url(&["search", "metadata"])), &body)?;
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
                        .with_context(|| format!("parsing the next page {next:?}"))
                })
                .transpose()?;
        }
        Ok(assets)
    }

    pub fn delete_assets(&self, ids: &[&str]) -> Result<()> {
        let body = json!({ "force": true, "ids": ids });
        self.send(with_body(self.client.delete(self.url(&["assets"])), &body)?)
    }

    pub fn restore_assets(&self, ids: &[&str]) -> Result<()> {
        let body = json!({ "ids": ids });
        let url = self.url(&["trash", "restore", "assets"]);
        self.send(with_body(self.client.post(url), &body)?)
    }

    pub fn libraries(&self) -> Result<Vec<Library>> {
        self.json(self.client.get(self.url(&["libraries"])))
    }

    pub fn scan_library(&self, id: &str) -> Result<()> {
        self.send(self.client.post(self.url(&["libraries", id, "scan"])))
    }

    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("the base url was checked to hold a path")
            .extend(segments);
        url
    }

    /// Sends a request and returns its body, failing on a non-success status.
    fn fetch(&self, request: RequestBuilder) -> Result<(String, String)> {
        let request = request.build().context("building a request")?;
        let target = format!("{} {}", request.method(), request.url());
        let response = self
            .client
            .execute(request)
            .with_context(|| format!("requesting {target}"))?;
        let status = response.status();
        let body = response
            .text()
            .with_context(|| format!("reading the response to {target}"))?;
        if !status.is_success() {
            bail!("{target} failed: {status} {body}");
        }
        Ok((target, body))
    }

    fn send(&self, request: RequestBuilder) -> Result<()> {
        self.fetch(request).map(drop)
    }

    fn json<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T> {
        let (target, body) = self.fetch(request)?;
        serde_json::from_str(&body).with_context(|| format!("decoding the response to {target}"))
    }
}

fn with_body(request: RequestBuilder, body: &Value) -> Result<RequestBuilder> {
    let body = serde_json::to_vec(body).context("encoding the request body")?;
    Ok(request.header(CONTENT_TYPE, "application/json").body(body))
}
