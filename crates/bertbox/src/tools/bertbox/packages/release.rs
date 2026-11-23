use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{Cursor, Read};

use anyhow::{Context, Result, anyhow, bail};
use bertbox_github::{Api, Error as GitHubError, Repository};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

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
    let repository = Repository {
        owner: repository.owner,
        name: repository.name,
    };
    let api = Api::new()?;

    let tag = latest_tag(&api, name, repository)?;
    let version = Version::Tag(tag.clone());
    let up_to_date = files::has_executables(name)?
        && Installation::load(name)?
            .is_some_and(|known| known.target == target && known.version == version);
    if !force && up_to_date {
        println!("Already up to date ({tag})");
        return files::link_package(name);
    }

    let expected = match checksum_asset {
        Some(checksum_asset) => Some(
            api.sha256(repository, &tag, &checksum_asset.name(&tag), asset)
                .with_context(|| format!("reading the checksum of {asset}"))?,
        ),
        None => None,
    };
    let archive = download::receive(
        api.asset(repository, &tag, asset)
            .with_context(|| format!("downloading {asset}"))?,
    )?;
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

fn latest_tag(api: &Api, name: PackageName, repository: Repository<'_>) -> Result<String> {
    api.latest_tag(repository).map_err(|error| match error {
        GitHubError::Moved { .. } => {
            anyhow!("{name} repository moved: {error}; update its owner and repository")
        }
        GitHubError::NoRelease { .. } => anyhow!("{name} has no published release: {error}"),
        error => anyhow::Error::new(error).context(format!("resolving the latest {name} release")),
    })
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
