//! A blocking client for the Forgejo repository API.

mod repo;

use std::num::NonZeroUsize;

use bertbox_http::{BaseUrl, Reply, json_body, sensitive_header};
use reqwest::blocking::{Client, ClientBuilder, RequestBuilder};
use reqwest::header::{AUTHORIZATION, HeaderMap};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub use bertbox_http::Error;
pub use repo::{ApiRepo, ParseRepoNameError, RepoName, Visibility};

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

type Inspect = Box<dyn Fn(&Value) + Send + Sync>;

pub struct Api {
    client: Client,
    base: BaseUrl,
    inspect: Option<Inspect>,
}

impl Api {
    /// Builds a client from `builder` that authenticates to the server at `base` with `token`.
    pub fn new(builder: ClientBuilder, base: &str, token: &str) -> Result<Self, Error> {
        let base = BaseUrl::parse(base, &["api", "v1"])?;
        let auth = sensitive_header("the token", &format!("token {token}"))?;
        let client = builder
            .default_headers(HeaderMap::from_iter([(AUTHORIZATION, auth)]))
            .build()
            .map_err(Error::Client)?;
        Ok(Self {
            client,
            base,
            inspect: None,
        })
    }

    /// Calls `inspect` with every successful JSON response before it is decoded.
    pub fn inspect(mut self, inspect: impl Fn(&Value) + Send + Sync + 'static) -> Self {
        self.inspect = Some(Box::new(inspect));
        self
    }

    /// Lists repos page by page, stopping once `limit` repos have arrived.
    pub fn list_repos(
        &self,
        owner: Option<&str>,
        limit: Option<NonZeroUsize>,
    ) -> Result<Vec<ApiRepo>, Error> {
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

    fn list_page(
        &self,
        owner: Option<&str>,
        page: u32,
        size: usize,
    ) -> Result<Vec<ApiRepo>, Error> {
        match owner {
            Some(owner) => {
                let url = self.base.join(["users", owner, "repos"]);
                self.json(self.client.get(paged(url, page, size)))
            }
            None => {
                let url = self.base.join(["repos", "search"]);
                let search: Search = self.json(self.client.get(paged(url, page, size)))?;
                Ok(search.data)
            }
        }
    }

    pub fn get_repo(&self, repo: &RepoName) -> Result<ApiRepo, Error> {
        self.json(self.client.get(self.repo_url(repo)))
    }

    pub fn create_repo(
        &self,
        repo: &RepoName,
        private: bool,
        description: &str,
    ) -> Result<ApiRepo, Error> {
        let user: User = self.json(self.client.get(self.base.join(["user"])))?;
        let url = if repo.owner == user.login {
            self.base.join(["user", "repos"])
        } else {
            self.base.join(["orgs", repo.owner.as_str(), "repos"])
        };
        let body = json!({
            "name": repo.name,
            "private": private,
            "description": description,
        });
        self.json(json_body(self.client.post(url), &body)?)
    }

    /// Deletes a repo, returning false when it does not exist.
    pub fn delete_repo(&self, repo: &RepoName) -> Result<bool, Error> {
        let reply = self.fetch(self.client.delete(self.repo_url(repo)))?;
        if reply.status == StatusCode::NOT_FOUND {
            return Ok(false);
        }
        reply.into_success()?;
        Ok(true)
    }

    /// Applies a partial update such as `{"archived": true}` to a repo.
    pub fn edit_repo(&self, repo: &RepoName, change: &Value) -> Result<ApiRepo, Error> {
        self.json(json_body(self.client.patch(self.repo_url(repo)), change)?)
    }

    fn repo_url(&self, repo: &RepoName) -> Url {
        self.base
            .join(["repos", repo.owner.as_str(), repo.name.as_str()])
    }

    fn fetch(&self, request: RequestBuilder) -> Result<Reply, Error> {
        bertbox_http::fetch(&self.client, request)
    }

    fn json<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T, Error> {
        let reply = self.fetch(request)?;
        let target = reply.target();
        let body = reply.into_success()?;
        let value: Value = bertbox_http::decode(&target, &body)?;
        if let Some(inspect) = &self.inspect {
            inspect(&value);
        }
        serde_json::from_value(value).map_err(|source| Error::Decode { target, source })
    }
}

fn paged(mut url: Url, page: u32, size: usize) -> Url {
    url.query_pairs_mut()
        .append_pair("limit", &size.to_string())
        .append_pair("page", &page.to_string());
    url
}
