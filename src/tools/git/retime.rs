use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use jiff::fmt::temporal::Pieces;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Zoned};

use crate::git::{self, Git};
use crate::tools::Tool;
use crate::tools::git::exit_code;

const ISO_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%:z";

#[derive(ClapArgs)]
#[command(
    about = "Rewrite the author and committer date of commits",
    after_help = "\
With no date option nothing is rewritten; the current date of each target is printed.

A range (any expression containing '..') is expanded via 'git rev-list'.
Note 'A..B' excludes A; use 'A^..B' or 'A~1..B' to include it."
)]
pub struct Retime {
    #[arg(value_name = "COMMIT", help = "commit or A..B range to retime")]
    pub commit: String,
    #[arg(
        short = 'n',
        long,
        help = "print '<shortsha> <newtime> <oldtime>' without rewriting"
    )]
    pub dry_run: bool,
    #[command(flatten)]
    pub date: DateOption,
}

#[derive(ClapArgs)]
pub struct DateOption {
    #[arg(
        long,
        value_name = "ISO-DATE",
        conflicts_with = "now",
        help = "absolute new date (ISO-8601)"
    )]
    pub set: Option<String>,
    #[arg(long, help = "set to the current local time")]
    pub now: bool,
    #[arg(long, value_name = "N", allow_negative_numbers = true, conflicts_with_all = ["set", "now"], help_heading = "Time shift", help = "add N seconds (may be negative)")]
    pub seconds: Option<i64>,
    #[arg(long, value_name = "N", allow_negative_numbers = true, conflicts_with_all = ["set", "now"], help_heading = "Time shift", help = "add N minutes (may be negative)")]
    pub minutes: Option<i64>,
    #[arg(long, value_name = "N", allow_negative_numbers = true, conflicts_with_all = ["set", "now"], help_heading = "Time shift", help = "add N hours (may be negative)")]
    pub hours: Option<i64>,
    #[arg(long, value_name = "N", allow_negative_numbers = true, conflicts_with_all = ["set", "now"], help_heading = "Time shift", help = "add N days (may be negative)")]
    pub days: Option<i64>,
    #[arg(long, value_name = "N", allow_negative_numbers = true, conflicts_with_all = ["set", "now"], help_heading = "Time shift", help = "add N weeks (may be negative)")]
    pub weeks: Option<i64>,
}

impl Tool for Retime {
    fn run(self) -> ExitCode {
        exit_code(run(self))
    }
}

impl DateOption {
    fn shift_seconds(&self) -> Result<Option<i64>> {
        let units = [
            (self.seconds, 1),
            (self.minutes, 60),
            (self.hours, 3600),
            (self.days, 86400),
            (self.weeks, 604800),
        ];
        let mut total = None;
        for (count, factor) in units {
            let Some(count) = count else { continue };
            total = Some(
                count
                    .checked_mul(factor)
                    .and_then(|seconds| seconds.checked_add(total.unwrap_or(0)))
                    .context("time shift is out of range")?,
            );
        }
        Ok(total)
    }

    /// The target date, or `None` when no date option was given.
    fn resolve(&self, old_date: &str) -> Result<Option<String>> {
        if let Some(shift) = self.shift_seconds()? {
            let new = parse_iso(old_date)?
                .checked_add(SignedDuration::from_secs(shift))
                .with_context(|| format!("shifting {old_date} by {shift} seconds"))?;
            return Ok(Some(new.strftime(ISO_FORMAT).to_string()));
        }
        if let Some(date) = &self.set {
            return Ok(Some(date.clone()));
        }
        if self.now {
            return Ok(Some(Zoned::now().strftime(ISO_FORMAT).to_string()));
        }
        Ok(None)
    }
}

fn parse_iso(date: &str) -> Result<Zoned> {
    let pieces = Pieces::parse(date).with_context(|| format!("parsing date {date:?}"))?;
    let (Some(time), Some(offset)) = (pieces.time(), pieces.to_numeric_offset()) else {
        bail!("date {date:?} has no time or offset");
    };
    pieces
        .date()
        .to_datetime(time)
        .to_zoned(TimeZone::fixed(offset))
        .with_context(|| format!("parsing date {date:?}"))
}

struct Target {
    old: String,
    date: Option<String>,
    old_date: String,
    height: u64,
}

impl Target {
    fn prepare(git: Git, commit: &str, date: &DateOption) -> Result<Self> {
        let old = capture(git, ["rev-parse", commit])?;
        let old_date = capture(git, ["show", "-s", "--format=%aI", &old])?;
        let date = date.resolve(&old_date)?;
        let height = capture(git, ["rev-list", "--count", &old])?
            .parse()
            .with_context(|| format!("counting the ancestors of {old}"))?;
        Ok(Self {
            old,
            date,
            old_date,
            height,
        })
    }
}

fn capture<const N: usize>(git: Git, args: [&str; N]) -> Result<String> {
    Ok(git.capture(args)?.trim().to_owned())
}

fn retime(git: Git, old: &str, date: &str, branch: &str) -> Result<()> {
    git::run(&mut git.command(["checkout", "-q", "--detach", old]))?;
    git::run(
        git.command([
            "commit",
            "-q",
            "--amend",
            "--no-edit",
            &format!("--date={date}"),
        ])
        .env("GIT_COMMITTER_DATE", date),
    )?;
    let new = capture(git, ["rev-parse", "HEAD"])?;
    git::run(&mut git.command(["checkout", "-q", branch]))?;
    git::run(&mut git.command(["rebase", "-q", "--onto", &new, old, branch]))
}

fn run(args: Retime) -> Result<()> {
    let git = Git::cwd();
    let commit = &args.commit;
    let commits: Vec<String> = if commit.contains("..") {
        let commits: Vec<String> = capture(git, ["rev-list", commit])?
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        if commits.is_empty() {
            bail!("range {commit:?} contains no commits");
        }
        commits
    } else {
        vec![commit.clone()]
    };
    let mut targets = commits
        .iter()
        .map(|commit| Target::prepare(git, commit, &args.date))
        .collect::<Result<Vec<_>>>()?;

    let will_write = !args.dry_run && targets.iter().any(|target| target.date.is_some());
    let branch = if will_write {
        let Ok(branch) = capture(git, ["symbolic-ref", "-q", "--short", "HEAD"]) else {
            bail!("HEAD is detached; check out a branch before retiming");
        };
        Some(branch)
    } else {
        None
    };

    targets.sort_by_key(|target| std::cmp::Reverse(target.height));
    for target in &targets {
        if let (Some(date), Some(branch)) = (&target.date, &branch) {
            retime(git, &target.old, date, branch)?;
        }
        let short = capture(git, ["rev-parse", "--short", &target.old])?;
        let date = target.date.as_deref().unwrap_or(&target.old_date);
        println!("{short} {date} {}", target.old_date);
    }
    Ok(())
}
