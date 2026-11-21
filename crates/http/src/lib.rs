//! Blocking HTTP plumbing shared by bertbox and its API clients.

use reqwest::blocking::{Client, ClientBuilder, RequestBuilder};
use reqwest::header::{CONTENT_TYPE, HeaderValue, InvalidHeaderValue};
use reqwest::{Method, StatusCode, Url};
use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("loading the TLS root certificates")]
    Certificates(#[source] reqwest::Error),
    #[error("parsing the url {url:?}")]
    ParseUrl {
        url: String,
        #[source]
        source: url::ParseError,
    },
    #[error("the url {url} cannot hold a path")]
    CannotHoldPath { url: Url },
    #[error("{what} is not a valid header value")]
    Header {
        what: &'static str,
        #[source]
        source: InvalidHeaderValue,
    },
    #[error("building the http client")]
    Client(#[source] reqwest::Error),
    #[error("building a request")]
    Build(#[source] reqwest::Error),
    #[error("encoding the request body")]
    Encode(#[source] serde_json::Error),
    #[error("requesting {target}")]
    Request {
        target: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("reading the response to {target}")]
    Read {
        target: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{target} failed: {status} {body}")]
    Status {
        target: String,
        status: StatusCode,
        body: String,
    },
    #[error("decoding the response to {target}")]
    Decode {
        target: String,
        #[source]
        source: serde_json::Error,
    },
}

/// Returns a blocking client builder sending `user_agent`, with TLS roots that work on every target.
pub fn client_builder(user_agent: &str) -> Result<ClientBuilder, Error> {
    let builder = ClientBuilder::new().user_agent(user_agent);
    // rustls-platform-verifier needs a JVM on Android, which a CLI does not have.
    #[cfg(target_os = "android")]
    let builder = builder.tls_certs_only(
        webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .map(|der| reqwest::Certificate::from_der(der))
            .collect::<reqwest::Result<Vec<_>>>()
            .map_err(Error::Certificates)?,
    );
    Ok(builder)
}

/// A URL known to hold a path, so path segments can always be appended to it.
#[derive(Clone, Debug)]
pub struct BaseUrl(Url);

impl BaseUrl {
    /// Parses `raw` and appends `segments` to its path, dropping any trailing slash first.
    pub fn parse(raw: &str, segments: &[&str]) -> Result<Self, Error> {
        let mut url = Url::parse(raw).map_err(|source| Error::ParseUrl {
            url: raw.to_owned(),
            source,
        })?;
        if url.cannot_be_a_base() {
            return Err(Error::CannotHoldPath { url });
        }
        url.path_segments_mut()
            .expect("a url that can be a base holds a path")
            .pop_if_empty()
            .extend(segments);
        Ok(Self(url))
    }

    /// The base URL with `segments` appended to its path.
    pub fn join<'a>(&self, segments: impl IntoIterator<Item = &'a str>) -> Url {
        let mut url = self.0.clone();
        url.path_segments_mut()
            .expect("a BaseUrl always holds a path")
            .extend(segments);
        url
    }
}

/// A header value hidden from debug output; `what` names it in the error.
pub fn sensitive_header(what: &'static str, value: &str) -> Result<HeaderValue, Error> {
    let mut header =
        HeaderValue::from_str(value).map_err(|source| Error::Header { what, source })?;
    header.set_sensitive(true);
    Ok(header)
}

/// Sets `body` as the request's JSON body.
pub fn json_body(request: RequestBuilder, body: &impl Serialize) -> Result<RequestBuilder, Error> {
    let body = serde_json::to_vec(body).map_err(Error::Encode)?;
    Ok(request.header(CONTENT_TYPE, "application/json").body(body))
}

/// A response whose body has been read in full.
pub struct Reply {
    pub method: Method,
    pub url: Url,
    pub status: StatusCode,
    pub body: String,
}

impl Reply {
    /// The `METHOD url` the reply answers, as errors name it.
    pub fn target(&self) -> String {
        target(&self.method, &self.url)
    }

    /// The body, or an error carrying it when the status is not a success.
    pub fn into_success(self) -> Result<String, Error> {
        if !self.status.is_success() {
            return Err(Error::Status {
                target: self.target(),
                status: self.status,
                body: self.body,
            });
        }
        Ok(self.body)
    }
}

/// Sends a request and reads its whole body, whatever the status.
pub fn fetch(client: &Client, request: RequestBuilder) -> Result<Reply, Error> {
    let request = request.build().map_err(Error::Build)?;
    let method = request.method().clone();
    let url = request.url().clone();
    let response = client.execute(request).map_err(|source| Error::Request {
        target: target(&method, &url),
        source,
    })?;
    let status = response.status();
    let body = response.text().map_err(|source| Error::Read {
        target: target(&method, &url),
        source,
    })?;
    Ok(Reply {
        method,
        url,
        status,
        body,
    })
}

/// Decodes a JSON `body`; `target` names the request in the error.
pub fn decode<T: DeserializeOwned>(target: &str, body: &str) -> Result<T, Error> {
    serde_json::from_str(body).map_err(|source| Error::Decode {
        target: target.to_owned(),
        source,
    })
}

fn target(method: &Method, url: &Url) -> String {
    format!("{method} {url}")
}
