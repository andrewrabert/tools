use anyhow::Result;
use reqwest::blocking::ClientBuilder;

use crate::{APP_NAME, APP_VERSION};

/// Returns a blocking client builder with bertbox's user agent and the shared TLS roots.
pub fn client_builder() -> Result<ClientBuilder> {
    Ok(bertbox_http::client_builder(&format!(
        "{APP_NAME}/{APP_VERSION}"
    ))?)
}
