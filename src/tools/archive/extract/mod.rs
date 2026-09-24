mod backend;
mod cli;
mod config;
mod dest;
mod detect;
mod listing;

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{self, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::tools::Tool;
pub use crate::tools::archive::extract::backend::seven_zip::program as seven_zip_program;
use crate::tools::archive::extract::backend::{Backend, Destination, Request, TEMP_PREFIX};
pub use crate::tools::archive::extract::cli::Extract;
pub use crate::tools::archive::extract::config::Names;
use crate::tools::archive::extract::config::{
    Action, ArchiveFate, Config, Credentials, Diagnostics, Extraction, Layout, OnError, Overwrite,
    Placement, Progress, Target,
};
pub use crate::tools::archive::extract::listing::Contents;
use crate::tools::archive::password::Password;
use crate::tools::archive::pool;
use crate::tools::archive::temp;

struct Job {
    backend: Backend,
    archive: PathBuf,
    volumes: Vec<PathBuf>,
}

impl Tool for Extract {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

pub fn list(archive: &Path) -> Result<Contents> {
    let archive =
        path::absolute(archive).with_context(|| format!("resolving {}", archive.display()))?;
    let backend = detect::backend(&archive)?;
    backend.check()?;
    backend.contents(&archive)
}

pub fn extract_to(archive: &Path, directory: &Path) -> Result<()> {
    let archive =
        path::absolute(archive).with_context(|| format!("resolving {}", archive.display()))?;
    let directory =
        path::absolute(directory).with_context(|| format!("resolving {}", directory.display()))?;
    let backend = detect::backend(&archive)?;
    backend.check()?;
    let volumes = backend.volumes(&archive, None)?;
    let job = Job {
        backend,
        archive,
        volumes,
    };
    let extraction = Extraction {
        target: Target::Directory {
            parent: Some(directory),
            placement: Placement::Merged,
        },
        members: Vec::new(),
        overwrite: Overwrite::Keep,
        archive_fate: ArchiveFate::Keep,
        progress: Progress::Silent,
        jobs: NonZeroUsize::MIN,
    };
    extract(&job, &extraction, None, &OnError::Fail)
}

pub fn extract_member(archive: &Path, member: &str, output: OwnedFd) -> Result<()> {
    let archive =
        path::absolute(archive).with_context(|| format!("resolving {}", archive.display()))?;
    let backend = detect::backend(&archive)?;
    backend.check()?;
    let volumes = backend.volumes(&archive, None)?;
    let job = Job {
        backend,
        archive,
        volumes,
    };
    let extraction = Extraction {
        target: Target::Stdout,
        members: vec![member.to_owned()],
        overwrite: Overwrite::Keep,
        archive_fate: ArchiveFate::Keep,
        progress: Progress::Silent,
        jobs: NonZeroUsize::MIN,
    };
    let work = Work {
        job: &job,
        extraction: &extraction,
        password: None,
    };
    work.extract_to(Destination::Stream(&output))
}

fn run(args: Extract) -> Result<ExitCode> {
    let config = Config::try_from(args)?;
    let password = match config.credentials {
        Credentials::Absent => None,
        Credentials::Prompt => Some(
            rpassword::prompt_password("Password: ")
                .context("reading the password")?
                .parse::<Password>()?,
        ),
    };

    let mut failed = Vec::new();
    let mut jobs = Vec::new();
    let mut claimed_volumes = HashSet::new();
    for archive in &config.archives {
        if claimed_volumes.contains(archive) {
            continue;
        }
        let backend = detect::backend(archive)?;
        if let Diagnostics::Debug = config.diagnostics {
            eprintln!("Using {backend:?} for {}", archive.display());
        }
        backend.check()?;

        if let Action::List { names, layout } = &config.action {
            let contents = backend.contents(archive)?;
            let mut stdout = io::stdout().lock();
            match layout {
                Layout::Json => writeln!(stdout, "{}", contents.json(names)?)?,
                Layout::Lines => stdout.write_all(&contents.lines(names))?,
            }
            stdout.flush()?;
            continue;
        }

        let volumes = match backend.volumes(archive, password.as_ref()) {
            Ok(volumes) => volumes,
            Err(error) => {
                eprintln!("Error: {error:?}");
                failed.push(archive.clone());
                continue;
            }
        };
        if let Action::Volumes = config.action {
            let mut listed: Vec<&[u8]> = volumes
                .iter()
                .map(|volume| volume.as_os_str().as_bytes())
                .collect();
            listed.sort();
            let mut stdout = io::stdout().lock();
            for volume in listed {
                stdout.write_all(volume)?;
                stdout.write_all(b"\n")?;
            }
            stdout.flush()?;
            continue;
        }
        claimed_volumes.extend(volumes.iter().cloned());
        jobs.push(Job {
            backend,
            archive: archive.clone(),
            volumes,
        });
    }

    if let Action::Extract(extraction) = &config.action {
        pool::run(
            jobs,
            extraction.jobs,
            |job| extract(job, extraction, password.as_ref(), &config.on_error),
            |job, extracted| {
                if let Err(error) = extracted {
                    eprintln!("Error: {error:?}");
                    failed.push(job.archive);
                }
            },
        );
    }

    failed.sort();
    for archive in &failed {
        eprintln!("Error extracting \"{}\"", archive.display());
    }
    Ok(match (failed.is_empty(), &config.on_error) {
        (true, _) | (false, OnError::Continue) => ExitCode::SUCCESS,
        (false, OnError::Fail) => ExitCode::FAILURE,
    })
}

struct Work<'a> {
    job: &'a Job,
    extraction: &'a Extraction,
    password: Option<&'a Password>,
}

impl Work<'_> {
    fn extract_to(&self, destination: Destination) -> Result<()> {
        let archive = self
            .job
            .volumes
            .first()
            .with_context(|| format!("{} has no volumes", self.job.archive.display()))?;
        self.job.backend.extract(&Request {
            archive,
            destination,
            members: &self.extraction.members,
            password: self.password,
            overwrite: self.extraction.overwrite,
        })
    }

    fn extract_staged(&self, dest: &Path, on_error: &OnError) -> Result<()> {
        let parent = dest
            .parent()
            .with_context(|| format!("{} has no parent", dest.display()))?;
        let name = dest
            .file_name()
            .with_context(|| format!("{} has no file name", dest.display()))?;
        let staging = temp::dir(parent, name, TEMP_PREFIX)?;
        let extracted = self.extract_to(Destination::Directory(staging.path()));
        match (extracted, on_error) {
            (Ok(()), _) => {
                let staged = staging.keep();
                fs::rename(&staged, dest).with_context(|| format!("moving to {}", dest.display()))
            }
            (Err(error), OnError::Continue) => {
                let _ = staging.keep();
                Err(error)
            }
            (Err(error), OnError::Fail) => Err(error),
        }
    }
}

fn extract(
    job: &Job,
    extraction: &Extraction,
    password: Option<&Password>,
    on_error: &OnError,
) -> Result<()> {
    if let Progress::Announced = extraction.progress {
        println!("Extracting {}", job.archive.display());
    }
    let work = Work {
        job,
        extraction,
        password,
    };
    match &extraction.target {
        Target::Stdout => {
            let stdout = io::stdout()
                .as_fd()
                .try_clone_to_owned()
                .context("duplicating stdout")?;
            work.extract_to(Destination::Stream(&stdout))?;
        }
        Target::Directory { parent, placement } => {
            let dest = dest::prepare(&job.archive, parent.as_deref(), placement)?;
            match placement {
                Placement::Merged => work.extract_to(Destination::Directory(&dest))?,
                Placement::ChildDirectory => work.extract_staged(&dest, on_error)?,
            }
        }
    }

    if let ArchiveFate::Remove = extraction.archive_fate {
        for volume in &job.volumes {
            fs::remove_file(volume).with_context(|| format!("removing {}", volume.display()))?;
        }
    }
    Ok(())
}
