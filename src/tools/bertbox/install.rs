use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::builder::PossibleValuesParser;
use clap::{Args as ClapArgs, ValueEnum};

use crate::tools::Tool;
use crate::{APP_NAME, Tools, dispatch};

#[derive(ClapArgs)]
#[command(about = "Link tool names to this binary")]
pub struct Install {
    /// Directory to create the links in
    #[arg(value_name = "DIR")]
    dir: PathBuf,

    /// Replace files that already exist
    #[arg(short, long)]
    force: bool,

    /// Kind of link to create
    #[arg(short, long, value_name = "KIND", default_value = "symlink")]
    link: Link,

    /// Install only this tool, along with any other selected (repeatable)
    #[arg(
        short,
        long = "tool",
        value_name = "TOOL",
        ignore_case = true,
        value_parser = PossibleValuesParser::new(tool_names())
    )]
    tools: Vec<String>,

    /// Install only this group's tools, along with any other selected (repeatable)
    #[arg(
        short,
        long = "group",
        value_name = "GROUP",
        ignore_case = true,
        value_parser = PossibleValuesParser::new(group_headings())
    )]
    groups: Vec<String>,

    /// Name each link bertbox-TOOL in place of TOOL
    #[arg(short, long)]
    prefixed: bool,
}

fn tool_names() -> Vec<&'static str> {
    Tools::GROUPS
        .iter()
        .flat_map(|group| group.names)
        .copied()
        .collect()
}

fn group_headings() -> Vec<&'static str> {
    Tools::GROUPS.iter().map(|group| group.heading).collect()
}

#[derive(Clone, Copy, ValueEnum)]
enum Link {
    Symlink,
    Hardlink,
}

impl Tool for Install {
    fn run(self) -> ExitCode {
        match self.install() {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

impl Install {
    /// Link the selected tools into the directory, returning whether none had to be skipped.
    fn install(&self) -> Result<bool> {
        let program = dispatch::program()?;
        let mut complete = true;
        for name in self.names() {
            let link = if self.prefixed {
                self.dir.join(format!("{APP_NAME}-{name}"))
            } else {
                self.dir.join(name)
            };
            if self.links(&link, &program) {
                continue;
            }
            if link.symlink_metadata().is_ok() {
                if !self.force {
                    eprintln!("Skipping {}: already exists", link.display());
                    complete = false;
                    continue;
                }
                fs::remove_file(&link).with_context(|| format!("removing {}", link.display()))?;
            }
            let linked = match self.link {
                Link::Symlink => symlink(&program, &link),
                Link::Hardlink => fs::hard_link(&program, &link),
            };
            linked
                .with_context(|| format!("linking {} to {}", link.display(), program.display()))?;
        }
        Ok(complete)
    }

    /// Names of the selected tools, or of every tool when nothing was selected.
    fn names(&self) -> impl Iterator<Item = &'static str> {
        let all = self.tools.is_empty() && self.groups.is_empty();
        Tools::GROUPS.iter().flat_map(move |group| {
            let grouped = all || selects(&self.groups, group.heading);
            group
                .names
                .iter()
                .copied()
                .filter(move |name| grouped || selects(&self.tools, name))
        })
    }

    /// Whether `link` is already the requested kind of link to `program`.
    fn links(&self, link: &Path, program: &Path) -> bool {
        match self.link {
            Link::Symlink => fs::read_link(link).is_ok_and(|current| current == program),
            Link::Hardlink => {
                let (Ok(link), Ok(program)) = (link.symlink_metadata(), program.metadata()) else {
                    return false;
                };
                link.dev() == program.dev() && link.ino() == program.ino()
            }
        }
    }
}

/// Whether `selected` holds `value`, ignoring case.
fn selects(selected: &[String], value: &str) -> bool {
    selected.iter().any(|item| item.eq_ignore_ascii_case(value))
}
