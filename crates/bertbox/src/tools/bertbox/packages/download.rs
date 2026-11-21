use std::io::{Cursor, Read};

use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::blocking::{Client, Response};
use reqwest::header::ETAG;
use reqwest::redirect::Policy;
use zip::ZipArchive;

use crate::http;

pub fn client() -> Result<Client> {
    http::client_builder()?
        .build()
        .context("building the HTTP client")
}

pub fn client_without_redirects() -> Result<Client> {
    http::client_builder()?
        .redirect(Policy::none())
        .build()
        .context("building the HTTP client")
}

pub fn get(client: &Client, url: &str) -> Result<Response> {
    client
        .get(url)
        .send()
        .and_then(Response::error_for_status)
        .with_context(|| format!("requesting {url}"))
}

pub fn etag(response: &Response) -> Result<String> {
    let value = response
        .headers()
        .get(ETAG)
        .with_context(|| format!("{} sent no ETag", response.url()))?;
    let value = value
        .to_str()
        .with_context(|| format!("{} sent a non-text ETag", response.url()))?;
    Ok(value.to_owned())
}

pub fn receive(response: Response) -> Result<Vec<u8>> {
    let url = response.url().to_string();
    let progress = match response.content_length() {
        Some(size) => ProgressBar::new(size),
        None => ProgressBar::new_spinner(),
    };
    progress.set_style(ProgressStyle::with_template(
        "Downloading {bar:40} {bytes}/{total_bytes} {bytes_per_sec} {eta}",
    )?);
    let mut body = Vec::new();
    progress
        .wrap_read(response)
        .read_to_end(&mut body)
        .with_context(|| format!("downloading {url}"))?;
    progress.finish_and_clear();
    Ok(body)
}

pub struct Member {
    pub name: String,
    pub data: Vec<u8>,
}

pub fn sole_member(archive: Vec<u8>) -> Result<Member> {
    let mut archive = ZipArchive::new(Cursor::new(archive)).context("reading the download")?;
    let mut files = Vec::new();
    for index in 0..archive.len() {
        if !archive.by_index(index)?.is_dir() {
            files.push(index);
        }
    }
    let [index] = files[..] else {
        bail!(
            "expected exactly one file in download, found {}",
            files.len()
        );
    };
    let mut file = archive.by_index(index)?;
    let name = file
        .name()
        .context("reading the download's file name")?
        .into_owned();
    let mut data = Vec::new();
    file.read_to_end(&mut data)
        .with_context(|| format!("extracting {name}"))?;
    Ok(Member { name, data })
}
