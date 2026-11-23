//! Blocking HTTP plumbing shared by bertbox and its API clients.

use std::io::{self, Read};

use reqwest::blocking::{Client, ClientBuilder, RequestBuilder, Response};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue, InvalidHeaderValue};
use reqwest::{Method, StatusCode, Url};
use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("loading the TLS root certificates")]
    Certificates(#[source] reqwest::Error),
    #[error("the url {url} cannot hold a path")]
    CannotHoldPath { url: Box<Url> },
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
    #[error("requesting {method} {url}")]
    Request {
        method: Method,
        url: Box<Url>,
        #[source]
        source: reqwest::Error,
    },
    #[error("reading the response to {method} {url}")]
    Read {
        method: Method,
        url: Box<Url>,
        #[source]
        source: reqwest::Error,
    },
    #[error("{method} {url} failed: {status} {body}")]
    Status {
        method: Method,
        url: Box<Url>,
        status: StatusCode,
        body: String,
    },
    #[error("decoding the response to {method} {url}")]
    Decode {
        method: Method,
        url: Box<Url>,
        #[source]
        source: serde_json::Error,
    },
}

/// The user agent every bertbox request sends.
pub const USER_AGENT: &str = concat!("bertbox/", env!("CARGO_PKG_VERSION"));

/// Returns a blocking client builder sending bertbox's user agent, with TLS roots that work on every target.
pub fn client_builder() -> Result<ClientBuilder, Error> {
    let builder = ClientBuilder::new().user_agent(USER_AGENT);
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
    /// Appends `segments` to the path of `url`, dropping any trailing slash first.
    pub fn new(mut url: Url, segments: &[&str]) -> Result<Self, Error> {
        if url.cannot_be_a_base() {
            return Err(Error::CannotHoldPath { url: Box::new(url) });
        }
        url.path_segments_mut()
            .expect("a url that can be a base holds a path")
            .pop_if_empty()
            .extend(segments);
        Ok(Self(url))
    }

    /// The base URL itself.
    pub fn url(&self) -> &Url {
        &self.0
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
    /// The URL that answered, after any redirects.
    pub url: Url,
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
}

impl Reply {
    /// The reply itself, or an error carrying its body when the status is not a success.
    pub fn into_success(self) -> Result<Self, Error> {
        if !self.status.is_success() {
            return Err(Error::Status {
                method: self.method,
                url: Box::new(self.url),
                status: self.status,
                body: self.body,
            });
        }
        Ok(self)
    }

    /// The body decoded from JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, Error> {
        serde_json::from_str(&self.body).map_err(|source| Error::Decode {
            method: self.method.clone(),
            url: Box::new(self.url.clone()),
            source,
        })
    }
}

/// Sends a request and reads its whole body, whatever the status.
pub fn fetch(client: &Client, request: RequestBuilder) -> Result<Reply, Error> {
    let request = request.build().map_err(Error::Build)?;
    let method = request.method().clone();
    let url = request.url().clone();
    let response = client.execute(request).map_err(|source| Error::Request {
        method: method.clone(),
        url: Box::new(url),
        source,
    })?;
    let url = response.url().clone();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().map_err(|source| Error::Read {
        method: method.clone(),
        url: Box::new(url.clone()),
        source,
    })?;
    Ok(Reply {
        method,
        url,
        status,
        headers,
        body,
    })
}

/// A successful response whose body has not been read yet, for streaming large files.
pub struct Download {
    response: Response,
}

impl Download {
    /// The URL the body came from, after any redirects.
    pub fn url(&self) -> &Url {
        self.response.url()
    }

    /// The body's size in bytes, when the server sent it.
    pub fn content_length(&self) -> Option<u64> {
        self.response.content_length()
    }

    /// A response header's value, when present and text.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.response.headers().get(name)?.to_str().ok()
    }
}

impl Read for Download {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.response.read(buf)
    }
}

/// Sends a GET for `url` and returns its unread body, failing on a non-success status.
pub fn download(client: &Client, url: Url) -> Result<Download, Error> {
    let response = client
        .get(url.clone())
        .send()
        .map_err(|source| Error::Request {
            method: Method::GET,
            url: Box::new(url),
            source,
        })?;
    let status = response.status();
    if !status.is_success() {
        let url = Box::new(response.url().clone());
        let body = response.text().map_err(|source| Error::Read {
            method: Method::GET,
            url: url.clone(),
            source,
        })?;
        return Err(Error::Status {
            method: Method::GET,
            url,
            status,
            body,
        });
    }
    Ok(Download { response })
}
