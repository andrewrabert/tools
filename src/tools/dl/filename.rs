use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use reqwest::header::CONTENT_DISPOSITION;

use crate::tools::dl::target::Target;
use crate::{APP_NAME, APP_VERSION};

pub enum Tls {
    Verified,
    Unverified,
}

pub fn suggested_by_server(target: &Target, tls: &Tls) -> Result<Option<PathBuf>> {
    let client = Client::builder()
        .tls_danger_accept_invalid_certs(matches!(tls, Tls::Unverified))
        .user_agent(format!("{APP_NAME}/{APP_VERSION}"))
        .build()
        .context("building the http client")?;
    let response = client
        .head(target.url().clone())
        .send()
        .with_context(|| format!("requesting headers of {target}"))?;
    let Some(disposition) = response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(None);
    };
    let disposition = mailparse::parse_content_disposition(disposition);
    Ok(disposition
        .params
        .get("filename")
        .and_then(|name| Path::new(name).file_name())
        .map(PathBuf::from))
}
