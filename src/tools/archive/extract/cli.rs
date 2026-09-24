use std::num::NonZeroUsize;
use std::path::{self, PathBuf};
use std::thread;

use anyhow::{Context, Error, Result, bail};
use clap::Args as ClapArgs;

use crate::tools::archive::extract::config::{
    Action, ArchiveFate, Config, Credentials, Diagnostics, Extraction, Layout, Names, OnError,
    Overwrite, Placement, Progress, Target,
};

#[derive(ClapArgs)]
#[command(about = "Extract archives in a variety of formats")]
pub struct Extract {
    #[arg(
        short = 'p',
        value_name = "DIR",
        help = "parent directory to extract to"
    )]
    parent: Option<PathBuf>,
    #[arg(
        short = 'c',
        conflicts_with = "stdout",
        help = "extract each archive to a unique child directory"
    )]
    child: bool,
    #[arg(long, help = "extract a specific file to stdout")]
    stdout: bool,
    #[arg(short = 'l', long, help = "list contents")]
    list: bool,
    #[arg(long, help = "output as json")]
    json: bool,
    #[arg(long, help = "output base64-encoded filenames")]
    base64: bool,
    #[arg(long, help = "list volumes")]
    volumes: bool,
    #[arg(short = 'f', long, help = "overwrite existing files/directories")]
    force: bool,
    #[arg(long, help = "remove archive after successful extraction")]
    rm: bool,
    #[arg(long, help = "prompt for password")]
    password: bool,
    #[arg(
        short = 'n',
        long = "num-concurrent",
        value_name = "NUM",
        default_value_t = 1,
        help = "number of parallel jobs, 0 for cpu count"
    )]
    num_procs: usize,
    #[arg(short = 'q', long, help = "suppress non-error output")]
    quiet: bool,
    #[arg(long)]
    verbose: bool,
    #[arg(long, help = "continue on extraction errors (leaves temp directories)")]
    ignore_errors: bool,
    #[arg(value_name = "ARCHIVE", required = true)]
    archives: Vec<PathBuf>,
}

impl TryFrom<Extract> for Config {
    type Error = Error;

    fn try_from(args: Extract) -> Result<Self> {
        let mut given = args.archives.into_iter();
        let (archives, members): (Vec<PathBuf>, Vec<String>) = if args.stdout {
            let archive = given.next().context("no archive given")?;
            let members = match (given.next(), given.next()) {
                (None, _) => Vec::new(),
                (Some(member), None) => vec![
                    member
                        .into_os_string()
                        .into_string()
                        .ok()
                        .context("the member to extract is not valid UTF-8")?,
                ],
                (Some(_), Some(_)) => {
                    bail!("--stdout takes one archive and at most one member")
                }
            };
            (vec![archive], members)
        } else {
            (given.collect(), Vec::new())
        };
        let archives: Vec<PathBuf> = archives
            .iter()
            .map(|archive| {
                path::absolute(archive).with_context(|| format!("resolving {}", archive.display()))
            })
            .collect::<Result<_>>()?;

        let action = if args.list {
            Action::List {
                names: if args.base64 {
                    Names::Base64
                } else {
                    Names::Raw
                },
                layout: if args.json {
                    Layout::Json
                } else {
                    Layout::Lines
                },
            }
        } else if args.volumes {
            Action::Volumes
        } else {
            Action::Extract(Extraction {
                target: if args.stdout {
                    Target::Stdout
                } else {
                    Target::Directory {
                        parent: match args.parent {
                            Some(parent) => Some(
                                path::absolute(&parent)
                                    .with_context(|| format!("resolving {}", parent.display()))?,
                            ),
                            None => None,
                        },
                        placement: if args.child {
                            Placement::ChildDirectory
                        } else {
                            Placement::Merged
                        },
                    }
                },
                members,
                overwrite: if args.force {
                    Overwrite::Replace
                } else {
                    Overwrite::Keep
                },
                archive_fate: if args.rm {
                    ArchiveFate::Remove
                } else {
                    ArchiveFate::Keep
                },
                progress: if args.quiet || args.stdout {
                    Progress::Silent
                } else {
                    Progress::Announced
                },
                jobs: match NonZeroUsize::new(args.num_procs) {
                    Some(jobs) => jobs,
                    None => thread::available_parallelism().context("counting cpus")?,
                },
            })
        };

        Ok(Config {
            archives,
            action,
            credentials: if args.password {
                Credentials::Prompt
            } else {
                Credentials::Absent
            },
            on_error: if args.ignore_errors {
                OnError::Continue
            } else {
                OnError::Fail
            },
            diagnostics: if args.verbose {
                Diagnostics::Debug
            } else {
                Diagnostics::Errors
            },
        })
    }
}
