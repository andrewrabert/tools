use std::io::{Cursor, Read};

use anyhow::{Context, Result, bail};
use bertbox_http::Download;
use indicatif::{ProgressBar, ProgressStyle};
use zip::ZipArchive;

/// Reads a whole download, showing its progress.
pub fn receive(download: Download) -> Result<Vec<u8>> {
    let url = download.url().clone();
    let progress = match download.content_length() {
        Some(size) => ProgressBar::new(size),
        None => ProgressBar::new_spinner(),
    };
    progress.set_style(ProgressStyle::with_template(
        "Downloading {bar:40} {bytes}/{total_bytes} {bytes_per_sec} {eta}",
    )?);
    let mut body = Vec::new();
    progress
        .wrap_read(download)
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
