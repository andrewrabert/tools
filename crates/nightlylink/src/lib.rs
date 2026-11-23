//! A blocking client for nightly.link, which locates the latest artifacts of CI workflows.

use reqwest::Url;
use reqwest::blocking::Client;
use reqwest::header::LOCATION;
use reqwest::redirect::Policy;

const ENDPOINT: &str = "https://nightly.link";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] bertbox_http::Error),
    #[error("{url} answered {status} without redirecting to the artifact")]
    NotRedirected { url: Box<Url>, status: u16 },
    #[error("{url} redirected to {location:?}, which is not a url")]
    Location {
        url: Box<Url>,
        location: String,
        #[source]
        source: url::ParseError,
    },
}

/// The URL nightly.link redirects to for downloading one artifact, a zip archive, of the
/// latest successful run of `workflow` on `branch`.
pub fn artifact_url(
    owner: &str,
    repository: &str,
    workflow: &str,
    branch: &str,
    artifact: &str,
) -> Result<Url, Error> {
    let client = client()?;
    let endpoint = Url::parse(ENDPOINT).expect("the endpoint is a valid url");
    let url = bertbox_http::BaseUrl::new(endpoint, &[])?.join([
        owner,
        repository,
        "workflows",
        workflow,
        branch,
        artifact,
    ]);
    let reply = bertbox_http::fetch(&client, client.get(url))?;
    let location = reply
        .headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok());
    match location {
        Some(location) if reply.status.is_redirection() => {
            reply.url.join(location).map_err(|source| Error::Location {
                url: Box::new(reply.url.clone()),
                location: location.to_owned(),
                source,
            })
        }
        _ => {
            let status = reply.status.as_u16();
            let reply = reply.into_success()?;
            Err(Error::NotRedirected {
                url: Box::new(reply.url),
                status,
            })
        }
    }
}

/// A client that reports redirects rather than follows them, since the redirect is the answer.
fn client() -> Result<Client, Error> {
    Ok(bertbox_http::client_builder()?
        .redirect(Policy::none())
        .build()
        .map_err(bertbox_http::Error::Client)?)
}
