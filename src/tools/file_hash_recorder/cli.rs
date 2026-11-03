use std::num::NonZeroUsize;
use std::path::PathBuf;

use anyhow::bail;
use clap::builder::{BoolishValueParser, RangedU64ValueParser, TypedValueParser};
use clap::{ArgAction, Args as ClapArgs};

use crate::tools::file_hash_recorder::archive::Extraction;
use crate::tools::file_hash_recorder::config::{
    Action, ArchivePolicy, Config, Database, Feedback, JsonStyle, Naming, OnError, Rehash,
    UpdateOptions,
};
use crate::tools::file_hash_recorder::locate::Missing;

#[derive(Clone, Copy)]
enum Mode {
    Update,
    List,
    Verify,
    Size,
    Dupes,
}

#[derive(ClapArgs)]
#[command(about = "Record file hashes in a file_info.db and query them")]
#[command(group = clap::ArgGroup::new("mode").multiple(false))]
pub struct FileHashRecorder {
    #[arg(
        long = "no-progress",
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => Feedback::Silent,
            false => Feedback::ProgressBar,
        })
    )]
    quiet: Feedback,
    #[arg(
        long,
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => Feedback::EachFile,
            false => Feedback::ProgressBar,
        })
    )]
    verbose: Feedback,
    #[arg(
        long = "absolute-paths",
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => Naming::Absolute,
            false => Naming::Relative,
        })
    )]
    naming: Naming,
    #[arg(
        short,
        long,
        group = "mode",
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => Mode::List,
            false => Mode::Update,
        })
    )]
    list: Mode,
    #[arg(
        long,
        group = "mode",
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => Mode::Verify,
            false => Mode::Update,
        })
    )]
    verify: Mode,
    #[arg(
        long,
        group = "mode",
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => Mode::Size,
            false => Mode::Update,
        }),
        help = "output total size of recorded files as '<bytes>\\t<human>\\t<path>'"
    )]
    size: Mode,
    #[arg(
        long,
        group = "mode",
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => Mode::Dupes,
            false => Mode::Update,
        })
    )]
    show_dupes: Mode,
    #[arg(
        short,
        long,
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => Rehash::Everything,
            false => Rehash::Changed,
        })
    )]
    force: Rehash,
    #[arg(
        long,
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => Missing::Create,
            false => Missing::Fail,
        }),
        help = "create the database if one does not already exist"
    )]
    mkdb: Missing,
    #[arg(
        short,
        long,
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => JsonStyle::Compact,
            false => JsonStyle::Pretty,
        }),
        help = "output compact json"
    )]
    compact: JsonStyle,
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
    #[arg(
        short = 'n',
        long = "num-concurrent",
        value_name = "NUM",
        default_value = "1",
        value_parser = RangedU64ValueParser::<usize>::new().try_map(crate::pool::jobs),
        help = "number of parallel jobs, 0 for cpu count"
    )]
    jobs: NonZeroUsize,
    #[arg(
        long = "no-archive-contents",
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => ArchivePolicy::Ignore,
            false => ArchivePolicy::Record(Extraction::TempDir),
        })
    )]
    archives: ArchivePolicy,
    #[arg(
        long = "skip-errors",
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => OnError::Skip,
            false => OnError::Abort,
        })
    )]
    on_error: OnError,
    #[arg(
        long = "no-tmpdir",
        action = ArgAction::SetTrue,
        value_parser = BoolishValueParser::new().map(|set| match set {
            true => Extraction::Stream,
            false => Extraction::TempDir,
        }),
        help = "don't extract to tempdir when hashing archive contents"
    )]
    extraction: Extraction,
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
}

impl TryFrom<FileHashRecorder> for Config {
    type Error = anyhow::Error;

    fn try_from(args: FileHashRecorder) -> Result<Self, Self::Error> {
        let mode = match (args.size, args.show_dupes, args.list, args.verify) {
            (Mode::Update, Mode::Update, Mode::Update, mode)
            | (Mode::Update, Mode::Update, mode, _)
            | (Mode::Update, mode, ..)
            | (mode, ..) => mode,
        };
        let database = args
            .output
            .map_or(Database::Discovered(args.mkdb), Database::Given);
        if args.paths.len() > 1 {
            match (&database, mode) {
                (Database::Given(_), _) | (Database::Discovered(_), Mode::Dupes | Mode::List) => {
                    bail!("--output, --show-dupes, and --list each take a single path")
                }
                (Database::Discovered(_), Mode::Update | Mode::Verify | Mode::Size) => {}
            }
        }

        let action = match mode {
            Mode::Size => Action::Size,
            Mode::Dupes => Action::Dupes,
            Mode::List => Action::List,
            Mode::Verify => Action::Verify,
            Mode::Update => Action::Update(UpdateOptions {
                rehash: args.force,
                archives: match args.archives {
                    ArchivePolicy::Ignore => ArchivePolicy::Ignore,
                    ArchivePolicy::Record(_) => ArchivePolicy::Record(args.extraction),
                },
                jobs: args.jobs,
                on_error: args.on_error,
            }),
        };

        Ok(Config {
            roots: args.paths,
            database,
            action,
            naming: args.naming,
            feedback: match (args.verbose, args.quiet) {
                (Feedback::EachFile, _) => Feedback::EachFile,
                (_, quiet) => quiet,
            },
            json: args.compact,
        })
    }
}
