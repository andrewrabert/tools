mod cli;
mod config;
mod format;
mod members;
pub(crate) mod report;
mod write;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::tools::Tool;
pub use crate::tools::archive::create::cli::Archive;
use crate::tools::archive::create::config::{Config, Placement, SourceFate};
use crate::tools::archive::create::format::Format;
use crate::tools::archive::create::report::SizeReport;
use crate::tools::archive::pool;
use crate::tools::archive::temp;
use crate::tools::archive::tree;

const TEMP_PREFIX: &str = ".archive_";

struct Archived {
    dest: PathBuf,
    before: u64,
    after: u64,
}

impl Tool for Archive {
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

fn run(args: Archive) -> Result<ExitCode> {
    let mut config = Config::try_from(args)?;
    let sources = std::mem::take(&mut config.sources);
    let mut report = SizeReport::new(sources.len());
    let mut code = ExitCode::SUCCESS;
    pool::run(
        sources,
        config.jobs,
        |source| archive(source, &config),
        |_, archived| match archived {
            Ok(archived) => report.print(archived.before, archived.after, &archived.dest),
            Err(error) => {
                eprintln!("Error: {error:?}");
                code = ExitCode::FAILURE;
            }
        },
    );
    report.print_total();
    Ok(code)
}

fn archive(source: &Path, config: &Config) -> Result<Archived> {
    let dest = match &config.placement {
        Placement::Exact(dest) => dest.clone(),
        Placement::Under(parent) => unused_dest(source, config.format, parent)?,
        Placement::BesideSource => {
            let parent = source
                .parent()
                .with_context(|| format!("{} has no parent", source.display()))?;
            unused_dest(source, config.format, parent)?
        }
    };
    let dest_parent = dest
        .parent()
        .with_context(|| format!("{} has no parent", dest.display()))?;
    let name = source
        .file_name()
        .with_context(|| format!("{} has no file name", source.display()))?;

    let mut before = 0;
    for path in tree::paths(source)? {
        before += fs::symlink_metadata(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .len();
    }

    let staged = temp::file(dest_parent, name, TEMP_PREFIX)?;
    write::write(
        config.format,
        source,
        &staged,
        config.speed,
        config.password.as_ref(),
    )
    .with_context(|| format!("archiving {}", source.display()))?;
    staged
        .persist(&dest)
        .with_context(|| format!("moving the archive to {}", dest.display()))?;
    let after = fs::symlink_metadata(&dest)
        .with_context(|| format!("reading {}", dest.display()))?
        .len();

    if let SourceFate::Remove = config.source_fate {
        let removed = if source.is_dir() {
            fs::remove_dir_all(source)
        } else {
            fs::remove_file(source)
        };
        removed.with_context(|| format!("removing {}", source.display()))?;
    }
    Ok(Archived {
        dest,
        before,
        after,
    })
}

fn unused_dest(source: &Path, format: Format, parent: &Path) -> Result<PathBuf> {
    let base = format.base_name(source);
    let mut increment = 0u64;
    loop {
        let mut name = OsString::from(&base);
        if increment > 0 {
            name.push(format!("_{increment}"));
        }
        name.push(format.suffix());
        let dest = parent.join(name);
        if !dest.exists() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
            return Ok(dest);
        }
        increment += 1;
    }
}
