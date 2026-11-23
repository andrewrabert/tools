use std::path::PathBuf;

use anyhow::{Context, Result};
use bertbox_http::Download;

use crate::install_target::InstallTarget;
use crate::tools::bertbox::packages::{GitHubRepository, Package, Version, download};

#[derive(Clone, Copy)]
pub struct NightlyArtifact {
    pub repository: GitHubRepository,
    pub workflow: &'static str,
    pub artifact: &'static str,
}

/// An artifact being downloaded, with the ETag that identifies its contents.
pub struct ArtifactDownload {
    pub etag: String,
    pub body: Download,
}

impl NightlyArtifact {
    /// Starts downloading the artifact from the workflow's latest run on `main`.
    pub fn download(&self) -> Result<ArtifactDownload> {
        let location = bertbox_nightlylink::artifact_url(
            self.repository.owner,
            self.repository.name,
            self.workflow,
            "main",
            self.artifact,
        )
        .with_context(|| format!("locating {}", self.artifact))?;
        let client = bertbox_http::client_builder()?
            .build()
            .context("building the HTTP client")?;
        let body = bertbox_http::download(&client, location)
            .with_context(|| format!("downloading {}", self.artifact))?;
        let etag = body
            .header("etag")
            .with_context(|| format!("{} sent no text ETag", body.url()))?
            .to_owned();
        Ok(ArtifactDownload { etag, body })
    }
}

pub fn deploy(
    package: &Package,
    target: InstallTarget,
    artifact: &NightlyArtifact,
    force: bool,
) -> Result<()> {
    let ArtifactDownload { etag, body } = artifact.download()?;
    let version = Version::Etag(etag);
    if !force && package.is_version(target, &version)? {
        println!("Already up to date");
        return package.link();
    }
    let archive = download::receive(body)?;

    println!("Extracting...");
    let binary = download::sole_member(archive)?.data;
    package.install(
        target,
        version,
        vec![(PathBuf::from(package.name()), binary)],
    )
}
