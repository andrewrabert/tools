use std::path::{self, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use crate::mkvmerge;
use crate::tools::Tool;
use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;

const MKV_EXTENSION: &str = "mkv";

#[derive(ClapArgs)]
#[command(about = "Set the title of Matroska files to their file name")]
pub struct MkvSetTitleFromFilename {
    /// Overwrite the title if already set
    #[arg(short, long)]
    force: bool,

    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
}

impl Tool for MkvSetTitleFromFilename {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: MkvSetTitleFromFilename) -> Result<()> {
    let files = args
        .paths
        .into_iter()
        .map(MkvFile::try_from)
        .collect::<Result<Vec<_>>>()?;
    for file in &files {
        match file.current_title()? {
            Some(title) if title == file.title => continue,
            Some(title) if !args.force => {
                bail!("{} already has the title {title:?}", file.path.display())
            }
            _ => {}
        }
        println!(
            "Setting title to \"{}\" ({})",
            file.title,
            file.path.display()
        );
        file.set_title()?;
    }
    Ok(())
}

struct MkvFile {
    // Absolute, since neither mkvmerge nor mkvpropedit accepts `--` to end its options.
    path: PathBuf,
    title: String,
}

impl TryFrom<PathBuf> for MkvFile {
    type Error = anyhow::Error;

    fn try_from(path: PathBuf) -> Result<Self> {
        if !path.is_file() {
            bail!("{} is not a file", path.display());
        }
        let is_mkv = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(MKV_EXTENSION));
        if !is_mkv {
            bail!("{} does not end with .{MKV_EXTENSION}", path.display());
        }
        let title = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .with_context(|| format!("the name of {} is not UTF-8", path.display()))?
            .to_owned();
        let path =
            path::absolute(&path).with_context(|| format!("resolving {}", path.display()))?;
        Ok(Self { path, title })
    }
}

impl MkvFile {
    fn current_title(&self) -> Result<Option<String>> {
        Ok(mkvmerge::identify(&self.path)?.container.properties.title)
    }

    fn set_title(&self) -> Result<()> {
        let status = Command::new("mkvpropedit")
            .args(["--edit", "info", "--set"])
            .arg(format!("title={}", self.title))
            .arg(&self.path)
            .stdout(Stdio::null())
            .status()
            .context("running mkvpropedit")?;
        if !status.success() {
            bail!("mkvpropedit failed: {status}");
        }
        Ok(())
    }
}
