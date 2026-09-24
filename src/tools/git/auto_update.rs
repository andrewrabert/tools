use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::git::{self, Git};
use crate::tools::Tool;
use crate::tools::git::exit_code;

#[derive(ClapArgs)]
#[command(about = "Merge upstream, commit every change, and push")]
pub struct AutoUpdate {
    #[arg(long, help = "commit only what is already staged")]
    pub staged: bool,
    #[arg(long, help = "never commit or push")]
    pub no_push: bool,
    #[arg(value_name = "REPO_DIR")]
    pub dir: Option<PathBuf>,
}

impl Tool for AutoUpdate {
    fn run(self) -> ExitCode {
        exit_code(run(self))
    }
}

fn run(args: AutoUpdate) -> Result<()> {
    let dir = match args.dir {
        Some(dir) => dir,
        None => {
            let mut status = Git::cwd().command(["status"]);
            if !status
                .stderr(Stdio::null())
                .status()
                .with_context(|| format!("running {status:?}"))?
                .success()
            {
                bail!("no git directory specified");
            }
            PathBuf::from(".")
        }
    };
    update(&dir, args.staged, args.no_push)
}

fn update(dir: &Path, staged: bool, no_push: bool) -> Result<()> {
    let git = Git::at(dir);
    git.run(["fetch"])?;
    if git.capture(["rev-parse", "HEAD"])? != git.capture(["rev-parse", "@{u}"])? {
        git.run(["merge"])?;
    }

    if !git.capture(["status", "--porcelain"])?.is_empty() {
        if no_push {
            eprintln!("warning: uncommitted changes");
        } else {
            if !staged {
                git.run(["add", "-A"])?;
            }
            let now = Timestamp::now().to_zoned(TimeZone::UTC);
            let message = format!("git-auto-update {}", now.strftime("%Y-%m-%dT%H:%M:%S%:z"));
            git::run(git.command(["commit", "-m"]).arg(message))?;
        }
    }

    let branch = git.capture(["rev-parse", "--abbrev-ref", "HEAD"])?;
    let branch = branch.trim_end_matches('\n');
    if !git
        .capture(["rev-list", &format!("origin/{branch}..{branch}")])?
        .is_empty()
    {
        if no_push {
            eprintln!("warning: local changes exist, but push is disabled");
        } else {
            git.run(["push"])?;
        }
    }
    Ok(())
}
