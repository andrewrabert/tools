use std::path::PathBuf;

use anyhow::bail;
use clap::Args as ClapArgs;

use crate::tools::file_hash_recorder::archive::Extraction;
use crate::tools::file_hash_recorder::config::{
    Action, ArchivePolicy, Config, Database, Feedback, JsonStyle, Mode, Naming, OnError, Rehash,
    UpdateOptions,
};
use crate::tools::file_hash_recorder::locate::Missing;

#[derive(ClapArgs)]
#[command(about = "Record file hashes in a file_info.db and query them")]
pub struct FileHashRecorder {
    #[arg(long)]
    no_progress: bool,
    #[arg(long)]
    verbose: bool,
    #[arg(long)]
    absolute_paths: bool,
    #[command(flatten)]
    mode: ModeArgs,
    #[arg(short, long)]
    force: bool,
    #[arg(long, help = "create the database if one does not already exist")]
    mkdb: bool,
    #[arg(short, long, help = "output compact json")]
    compact: bool,
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
    #[arg(
        short,
        long = "num-concurrent",
        value_name = "NUM",
        default_value_t = 1,
        help = "number of parallel jobs, 0 for cpu count"
    )]
    num_procs: usize,
    #[arg(long)]
    no_archive_contents: bool,
    #[arg(long)]
    skip_errors: bool,
    #[arg(long, help = "don't extract to tempdir when hashing archive contents")]
    no_tmpdir: bool,
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
}

#[derive(ClapArgs)]
#[group(multiple = false)]
struct ModeArgs {
    #[arg(short, long)]
    list: bool,
    #[arg(long)]
    verify: bool,
    #[arg(long)]
    multihash: bool,
    #[arg(
        long,
        help = "output total size of recorded files as '<bytes>\\t<human>\\t<path>'"
    )]
    size: bool,
    #[arg(long)]
    show_dupes: bool,
}

impl TryFrom<FileHashRecorder> for Config {
    type Error = anyhow::Error;

    fn try_from(args: FileHashRecorder) -> Result<Self, Self::Error> {
        let single_root_only =
            args.output.is_some() || args.mode.show_dupes || args.mode.list || args.mode.multihash;
        if args.paths.len() > 1 && single_root_only {
            bail!("Cannot use --output, --show-dupes, --list, or --multihash with multiple paths");
        }

        let action = match args.mode {
            ModeArgs {
                multihash: true, ..
            } => None,
            ModeArgs { size: true, .. } => Some(Action::Size),
            ModeArgs {
                show_dupes: true, ..
            } => Some(Action::Dupes),
            ModeArgs { list: true, .. } => Some(Action::List),
            ModeArgs { verify: true, .. } => Some(Action::Verify),
            ModeArgs { .. } => Some(Action::Update(UpdateOptions {
                rehash: if args.force {
                    Rehash::Everything
                } else {
                    Rehash::Changed
                },
                archives: match (args.no_archive_contents, args.no_tmpdir) {
                    (true, _) => ArchivePolicy::Ignore,
                    (false, true) => ArchivePolicy::Record(Extraction::Stream),
                    (false, false) => ArchivePolicy::Record(Extraction::TempDir),
                },
                jobs: crate::pool::jobs(args.num_procs)?,
                on_error: if args.skip_errors {
                    OnError::Skip
                } else {
                    OnError::Abort
                },
            })),
        };
        let database = match (args.output, args.mkdb) {
            (Some(path), _) => Database::Given(path),
            (None, true) => Database::Discovered(Missing::Create),
            (None, false) => Database::Discovered(Missing::Fail),
        };
        let mode = match action {
            None => Mode::Multihash,
            Some(action) => Mode::Recorded { database, action },
        };

        Ok(Config {
            roots: args.paths,
            mode,
            naming: if args.absolute_paths {
                Naming::Absolute
            } else {
                Naming::Relative
            },
            feedback: if args.verbose {
                Feedback::EachFile
            } else if args.no_progress {
                Feedback::Silent
            } else {
                Feedback::ProgressBar
            },
            json: if args.compact {
                JsonStyle::Compact
            } else {
                JsonStyle::Pretty
            },
        })
    }
}
