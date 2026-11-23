use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use clap::builder::PossibleValuesParser;
use clap::{Args as ClapArgs, ValueEnum};

use crate::tools::Tool;
use crate::tools::bertbox::packages;
use crate::{APP_NAME, Tools, dispatch};

#[derive(ClapArgs)]
#[command(about = "Link tool names to this binary")]
pub struct Link {
    /// Directory to create the links in [default: the shared bin directory]
    #[arg(value_name = "DIR")]
    dir: Option<PathBuf>,

    /// Replace files that already exist
    #[arg(short, long)]
    force: bool,

    /// Kind of link to create
    #[arg(short, long, value_name = "KIND", default_value = "symlink")]
    link: LinkKind,

    /// Install only this tool, along with any other selected (repeatable). The External group
    /// is installed only when selected
    #[arg(
        short,
        long = "tool",
        value_name = "TOOL",
        ignore_case = true,
        value_parser = PossibleValuesParser::new(tool_names())
    )]
    tools: Vec<String>,

    /// Install only this group's tools, along with any other selected (repeatable). The External
    /// group is installed only when selected
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

    /// Replace DIR's contents with the links, keeping directories and replacing existing files
    #[arg(long)]
    replace: bool,
}

const OPT_IN_GROUPS: &[&str] = &["External"];

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

pub fn default_names() -> impl Iterator<Item = &'static str> {
    Tools::GROUPS
        .iter()
        .filter(|group| !OPT_IN_GROUPS.contains(&group.heading))
        .flat_map(|group| group.names)
        .copied()
}

#[derive(Clone, Copy, ValueEnum)]
enum LinkKind {
    Symlink,
    Hardlink,
}

impl Tool for Link {
    fn run(self) -> ExitCode {
        match self.link() {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

impl Link {
    fn link(&self) -> Result<bool> {
        let dir = match &self.dir {
            Some(dir) => dir.clone(),
            None => {
                let dir = packages::bin_dir()?;
                fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
                dir
            }
        };
        let program = dispatch::program()?;
        let mut complete = true;
        let mut installed = HashSet::new();
        for name in self.names() {
            let file_name = if self.prefixed {
                OsString::from(format!("{APP_NAME}-{name}"))
            } else {
                OsString::from(name)
            };
            let link = dir.join(&file_name);
            installed.insert(file_name);
            if self.links(&link, &program) {
                continue;
            }
            if link.symlink_metadata().is_ok() {
                if !self.force && !self.replace {
                    eprintln!("Skipping {}: already exists", link.display());
                    complete = false;
                    continue;
                }
                fs::remove_file(&link).with_context(|| format!("removing {}", link.display()))?;
            }
            let linked = match self.link {
                LinkKind::Symlink => symlink(&program, &link),
                LinkKind::Hardlink => fs::hard_link(&program, &link),
            };
            linked
                .with_context(|| format!("linking {} to {}", link.display(), program.display()))?;
        }
        if self.replace && !prune(&dir, &installed, &program)? {
            complete = false;
        }
        write_completions(&program)?;
        Ok(complete)
    }

    fn names(&self) -> Box<dyn Iterator<Item = &'static str> + '_> {
        if self.tools.is_empty() && self.groups.is_empty() {
            return Box::new(default_names());
        }
        Box::new(Tools::GROUPS.iter().flat_map(move |group| {
            let grouped = selects(&self.groups, group.heading);
            group
                .names
                .iter()
                .copied()
                .filter(move |name| grouped || selects(&self.tools, name))
        }))
    }

    fn links(&self, link: &Path, program: &Path) -> bool {
        match self.link {
            LinkKind::Symlink => fs::read_link(link).is_ok_and(|current| current == program),
            LinkKind::Hardlink => {
                let (Ok(link), Ok(program)) = (link.symlink_metadata(), program.metadata()) else {
                    return false;
                };
                link.dev() == program.dev() && link.ino() == program.ino()
            }
        }
    }
}

fn write_completions(program: &Path) -> Result<()> {
    write_shell_completions(
        program,
        "fish",
        &packages::fish_dir()?.join(format!("{APP_NAME}.fish")),
    )?;
    write_shell_completions(
        program,
        "zsh",
        &packages::zsh_dir()?.join(format!("_{APP_NAME}")),
    )
}

fn write_shell_completions(program: &Path, shell: &str, dest: &Path) -> Result<()> {
    let output = Command::new(program)
        .env(dispatch::ENV_NAME, dispatch::ENV_VALUE)
        .args(["completions", "--all", shell])
        .output()
        .with_context(|| format!("running {}", program.display()))?;
    if !output.status.success() {
        bail!(
            "{} {shell} completions failed: {}",
            program.display(),
            output.status
        );
    }
    packages::write_file(dest, &output.stdout)?;
    println!("Installed: {}", dest.display());
    Ok(())
}

fn prune(dir: &Path, installed: &HashSet<OsString>, program: &Path) -> Result<bool> {
    let resolved = fs::canonicalize(dir).with_context(|| format!("resolving {}", dir.display()))?;
    let entries = fs::read_dir(&resolved).with_context(|| format!("reading {}", dir.display()))?;
    let mut complete = true;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let path = entry.path();
        if installed.contains(&entry.file_name()) || path == program {
            continue;
        }
        let file_type = entry
            .file_type()
            .with_context(|| format!("inspecting {}", path.display()))?;
        if file_type.is_dir() {
            eprintln!("Skipping {}: is a directory", path.display());
            complete = false;
            continue;
        }
        fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(complete)
}

fn selects(selected: &[String], value: &str) -> bool {
    selected.iter().any(|item| item.eq_ignore_ascii_case(value))
}
