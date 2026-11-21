use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{Cursor, Read};

use anyhow::{Context, Result, anyhow, bail};
use flate2::read::GzDecoder;
use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::LOCATION;
use sha2::{Digest, Sha256};
use url::Url;

use crate::install_target::InstallTarget;
use crate::tools::bertbox::packages::{
    Archive, Executables, GitHubRepository, Installation, PackageName, Version, download, files,
};

#[derive(Clone, Copy)]
pub enum ChecksumAsset {
    Fixed(&'static str),
    PerTag(fn(tag: &str) -> String),
}

impl ChecksumAsset {
    fn name(&self, tag: &str) -> String {
        match self {
            ChecksumAsset::Fixed(name) => (*name).to_owned(),
            ChecksumAsset::PerTag(name) => name(tag),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Release {
    pub repository: GitHubRepository,
    pub asset: &'static str,
    pub archive: Archive,
    pub checksum_asset: Option<ChecksumAsset>,
    pub executables: Executables,
}

pub fn deploy(
    name: PackageName,
    target: InstallTarget,
    release: &Release,
    force: bool,
) -> Result<()> {
    let Release {
        repository,
        asset,
        archive: format,
        checksum_asset,
        executables,
    } = *release;
    let releases = format!("https://github.com/{repository}/releases");

    let tag = latest_tag(name, repository, &releases)?;
    let version = Version::Tag(tag.clone());
    let up_to_date = files::has_executables(name)?
        && Installation::load(name)?
            .is_some_and(|known| known.target == target && known.version == version);
    if !force && up_to_date {
        println!("Already up to date ({tag})");
        return files::link_package(name);
    }

    let client = download::client()?;
    let expected = match checksum_asset {
        Some(checksum_asset) => Some(expected_sha256(
            &client,
            &releases,
            &tag,
            &checksum_asset.name(&tag),
            asset,
        )?),
        None => None,
    };
    let url = format!("{releases}/download/{tag}/{asset}");
    let archive = download::receive(download::get(&client, &url)?)?;
    if let Some(expected) = expected {
        let digest = hex::encode(Sha256::digest(&archive));
        if digest != expected {
            bail!("{asset}: expected {expected}, got {digest}");
        }
    }

    println!("Extracting...");
    let mut written = HashSet::new();
    each_file(&archive, format, |file_name, contents| {
        let wanted = match executables {
            Executables::Named => file_name == name.as_str() && written.is_empty(),
            Executables::All => true,
        };
        if wanted {
            let mut data = Vec::new();
            contents
                .read_to_end(&mut data)
                .with_context(|| format!("extracting {file_name}"))?;
            files::write_executable(name, file_name, &data)?;
            written.insert(OsString::from(file_name));
        }
        Ok(())
    })?;
    if written.is_empty() {
        bail!("no {name} executable in download");
    }
    files::remove_other_executables(name, &written)?;
    println!("Installed ({tag})");
    files::link_package(name)?;
    Installation {
        target,
        version,
        sha256: None,
    }
    .store(name)
}

fn latest_tag(name: PackageName, repository: GitHubRepository, releases: &str) -> Result<String> {
    let url = format!("{releases}/latest");
    let response = download::client_without_redirects()?
        .head(&url)
        .send()
        .with_context(|| format!("requesting {url}"))?;
    let status = response.status();
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let moved = |location: &str| {
        format!(
            "{name} repository moved: {url} now redirects to {location}; \
             update its owner and repository"
        )
    };
    if status == StatusCode::MOVED_PERMANENTLY {
        bail!(moved(location.as_deref().unwrap_or("nowhere")));
    }
    let (StatusCode::FOUND, Some(location)) = (status, location) else {
        bail!("could not resolve latest {name} release: HTTP {status} from {url}");
    };
    let Some((found_owner, found_repository, tag)) = release_tag(&location) else {
        bail!("{name} has no published release: {url} redirects to {location}");
    };
    if (found_owner.as_str(), found_repository.as_str()) != (repository.owner, repository.name) {
        bail!(moved(&location));
    }
    Ok(tag)
}

fn release_tag(location: &str) -> Option<(String, String, String)> {
    let url = Url::parse(location).ok()?;
    if url.host_str() != Some("github.com") {
        return None;
    }
    let segments: Vec<&str> = url.path_segments()?.collect();
    let [owner, repository, "releases", "tag", ref tag @ ..] = segments[..] else {
        return None;
    };
    if tag.is_empty() || tag.iter().any(|part| part.is_empty()) {
        return None;
    }
    Some((owner.to_owned(), repository.to_owned(), tag.join("/")))
}

fn expected_sha256(
    client: &Client,
    releases: &str,
    tag: &str,
    checksums: &str,
    asset: &str,
) -> Result<String> {
    let url = format!("{releases}/download/{tag}/{checksums}");
    let text = download::get(client, &url)?
        .text()
        .with_context(|| format!("reading {url}"))?;
    text.lines()
        .filter_map(checksum_line)
        .find(|(_, name)| *name == asset)
        .map(|(digest, _)| digest.to_owned())
        .with_context(|| format!("no checksum published for {asset}"))
}

fn checksum_line(line: &str) -> Option<(&str, &str)> {
    let (digest, rest) = line.split_once(' ')?;
    let name = rest.strip_prefix(' ').or_else(|| rest.strip_prefix('*'))?;
    Some((digest, name))
}

fn each_file(
    archive: &[u8],
    format: Archive,
    mut each: impl FnMut(&str, &mut dyn Read) -> Result<()>,
) -> Result<()> {
    let tar: Box<dyn Read + '_> = match format {
        Archive::TarGz => Box::new(GzDecoder::new(Cursor::new(archive))),
        Archive::TarXz => {
            let mut tar = Vec::new();
            lzma_rs::xz_decompress(&mut Cursor::new(archive), &mut tar)
                .map_err(|error| anyhow!("decompressing the download: {error}"))?;
            Box::new(Cursor::new(tar))
        }
    };
    let mut tar = tar::Archive::new(tar);
    for entry in tar.entries().context("reading the download")? {
        let mut entry = entry.context("reading the download")?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let file_name = entry
            .path()
            .context("reading the download")?
            .file_name()
            .and_then(|file| file.to_str())
            .map(str::to_owned);
        if let Some(file_name) = file_name {
            each(&file_name, &mut entry)?;
        }
    }
    Ok(())
}
