use std::num::NonZeroUsize;
use std::path::{self, PathBuf};
use std::thread;

use anyhow::{Context, Error, Result, bail};
use clap::Args as ClapArgs;

use crate::tools::archive::create::config::{Config, Placement, SourceFate, Speed};
use crate::tools::archive::create::format::Format;
use crate::tools::archive::password::Password;

#[derive(ClapArgs)]
#[command(about = "Create archives in a variety of formats")]
pub struct Archive {
    #[arg(
        short = 'f',
        value_name = "FORMAT",
        default_value = "7z",
        help = "archive format"
    )]
    format: Format,
    #[arg(long, help = "increase archiving speed at the expense of compression")]
    fast: bool,
    #[arg(long, help = "remove source after success")]
    rm: bool,
    #[arg(
        short = 'n',
        long = "num-concurrent",
        value_name = "NUM",
        default_value_t = 1,
        help = "number of parallel jobs, 0 for cpu count"
    )]
    num_procs: usize,
    #[arg(
        short = 'p',
        value_name = "DIR",
        help = "parent directory to save the archive in"
    )]
    parent: Option<PathBuf>,
    #[arg(long, help = "archive password")]
    password: Option<Password>,
    #[arg(short = 'o', long, value_name = "FILE", help = "output filename")]
    output: Option<PathBuf>,
    #[arg(value_name = "SOURCE", required = true)]
    sources: Vec<PathBuf>,
}

impl TryFrom<Archive> for Config {
    type Error = Error;

    fn try_from(args: Archive) -> Result<Self> {
        let mut sources = Vec::new();
        for source in &args.sources {
            let source = path::absolute(source)
                .with_context(|| format!("resolving {}", source.display()))?;
            if !sources.contains(&source) {
                sources.push(source);
            }
        }
        if args.password.is_some() && !args.format.protects_with_password() {
            bail!("the chosen format cannot be password protected");
        }
        let placement = match (args.output, args.parent) {
            (Some(output), _) => {
                if args.sources.len() != 1 {
                    bail!("may only specify -o/--output when archiving a single source");
                }
                if output.exists() {
                    bail!("-o/--output path already exists");
                }
                Placement::Exact(
                    path::absolute(&output)
                        .with_context(|| format!("resolving {}", output.display()))?,
                )
            }
            (None, Some(parent)) => Placement::Under(
                path::absolute(&parent)
                    .with_context(|| format!("resolving {}", parent.display()))?,
            ),
            (None, None) => Placement::BesideSource,
        };
        Ok(Config {
            sources,
            format: args.format,
            speed: if args.fast { Speed::Fast } else { Speed::Best },
            placement,
            password: args.password,
            source_fate: if args.rm {
                SourceFate::Remove
            } else {
                SourceFate::Keep
            },
            jobs: match NonZeroUsize::new(args.num_procs) {
                Some(jobs) => jobs,
                None => thread::available_parallelism().context("counting cpus")?,
            },
        })
    }
}
