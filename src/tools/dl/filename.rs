use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use reqwest::header::CONTENT_DISPOSITION;

use crate::http;
use crate::tools::dl::target::Target;

pub enum Tls {
    Verified,
    Unverified,
}

pub fn suggested_by_server(target: &Target, tls: &Tls) -> Result<Option<PathBuf>> {
    let client = http::client_builder()?
        .tls_danger_accept_invalid_certs(matches!(tls, Tls::Unverified))
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
