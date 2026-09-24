use std::num::NonZeroUsize;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::tools::forgejo::repo::{ApiRepo, RepoName};
use crate::{APP_NAME, APP_VERSION};

const PAGE_SIZE: usize = 50;

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    data: Vec<ApiRepo>,
}

/// A response whose body has been read in full.
struct Reply {
    target: String,
    status: StatusCode,
    body: String,
}

impl Reply {
    fn into_success(self) -> Result<String> {
        if !self.status.is_success() {
            bail!("{} failed: {} {}", self.target, self.status, self.body);
        }
        Ok(self.body)
    }
}

pub struct Api {
    client: Client,
    base: Url,
    verbose: bool,
}

impl Api {
    pub fn new(base: &str, token: &str, verbose: bool) -> Result<Self> {
        let mut base = Url::parse(base).with_context(|| format!("parsing the url {base:?}"))?;
        base.path_segments_mut()
            .map_err(|()| anyhow!("the forgejo url cannot hold a path"))?
            .pop_if_empty()
            .extend(["api", "v1"]);
        let mut auth = HeaderValue::from_str(&format!("token {token}"))
            .context("the token is not a valid header value")?;
        auth.set_sensitive(true);
        let client = Client::builder()
            .user_agent(format!("{APP_NAME}/{APP_VERSION}"))
            .default_headers(HeaderMap::from_iter([(AUTHORIZATION, auth)]))
            .build()
            .context("building the http client")?;
        Ok(Self {
            client,
            base,
            verbose,
        })
    }

    /// Lists repos page by page, stopping once `limit` repos have arrived.
    pub fn list_repos(
        &self,
        owner: Option<&str>,
        limit: Option<NonZeroUsize>,
    ) -> Result<Vec<ApiRepo>> {
        let limit = limit.map_or(usize::MAX, NonZeroUsize::get);
        // One page size for every request, so page numbers keep their offsets.
        let size = limit.min(PAGE_SIZE);
        let mut repos = Vec::new();
        // The server may cap a page below `size`, so only an empty page ends the list.
        for page in 1u32.. {
            let batch = self.list_page(owner, page, size)?;
            if batch.is_empty() {
                break;
            }
            repos.extend(batch);
            if repos.len() >= limit {
                repos.truncate(limit);
                break;
            }
        }
        Ok(repos)
    }

    fn list_page(&self, owner: Option<&str>, page: u32, size: usize) -> Result<Vec<ApiRepo>> {
        match owner {
            Some(owner) => {
                let url = self.url(&["users", owner, "repos"]);
                self.json(self.client.get(paged(url, page, size)))
            }
            None => {
                let url = self.url(&["repos", "search"]);
                let search: Search = self.json(self.client.get(paged(url, page, size)))?;
                Ok(search.data)
            }
        }
    }

    pub fn get_repo(&self, repo: &RepoName) -> Result<ApiRepo> {
        self.json(self.client.get(self.repo_url(repo)))
    }

    pub fn create_repo(
        &self,
        repo: &RepoName,
        private: bool,
        description: &str,
    ) -> Result<ApiRepo> {
        let user: User = self.json(self.client.get(self.url(&["user"])))?;
        let url = if repo.owner == user.login {
            self.url(&["user", "repos"])
        } else {
            self.url(&["orgs", &repo.owner, "repos"])
        };
        let body = json!({
            "name": repo.name,
            "private": private,
            "description": description,
        });
        self.json(with_body(self.client.post(url), &body)?)
    }

    /// Deletes a repo, returning false when it does not exist.
    pub fn delete_repo(&self, repo: &RepoName) -> Result<bool> {
        let reply = self.fetch(self.client.delete(self.repo_url(repo)))?;
        if reply.status == StatusCode::NOT_FOUND {
            return Ok(false);
        }
        reply.into_success()?;
        Ok(true)
    }

    /// Applies a partial update such as `{"archived": true}` to a repo.
    pub fn edit_repo(&self, repo: &RepoName, change: &Value) -> Result<ApiRepo> {
        self.json(with_body(self.client.patch(self.repo_url(repo)), change)?)
    }

    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("the base url was checked to hold a path")
            .extend(segments);
        url
    }

    fn repo_url(&self, repo: &RepoName) -> Url {
        self.url(&["repos", &repo.owner, &repo.name])
    }

    fn fetch(&self, request: RequestBuilder) -> Result<Reply> {
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
        Ok(Reply {
            target,
            status,
            body,
        })
    }

    fn json<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T> {
        let reply = self.fetch(request)?;
        let target = reply.target.clone();
        let body = reply.into_success()?;
        let value: Value = serde_json::from_str(&body)
            .with_context(|| format!("parsing the response to {target}"))?;
        if self.verbose {
            // serde_json keeps object keys sorted.
            eprintln!("{}", serde_json::to_string_pretty(&value)?);
        }
        serde_json::from_value(value).with_context(|| format!("decoding the response to {target}"))
    }
}

fn paged(mut url: Url, page: u32, size: usize) -> Url {
    url.query_pairs_mut()
        .append_pair("limit", &size.to_string())
        .append_pair("page", &page.to_string());
    url
}

fn with_body(request: RequestBuilder, body: &Value) -> Result<RequestBuilder> {
    let body = serde_json::to_vec(body).context("encoding the request body")?;
    Ok(request.header(CONTENT_TYPE, "application/json").body(body))
}
