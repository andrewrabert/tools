use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Args as ClapArgs;

use crate::git::Git;
use crate::tools::Tool;
use crate::tools::git::exit_code;

#[derive(ClapArgs)]
#[command(about = "Check that a tag and every commit it holds are pushed")]
pub struct CheckTagIsPushed {
    #[arg(long, env = "GIT_REMOTE", default_value = "origin")]
    pub remote: String,
    #[arg(long, env = "GIT_BRANCH", default_value = "main")]
    pub branch: String,
    #[arg(value_name = "TAG")]
    pub tag: String,
}

impl Tool for CheckTagIsPushed {
    fn run(self) -> ExitCode {
        exit_code(run(&self))
    }
}

fn run(args: &CheckTagIsPushed) -> Result<()> {
    let git = Git::cwd();
    let tag_ref = format!("refs/tags/{}", args.tag);

    let local = if git.succeeds(["show-ref", "--verify", "--quiet", &tag_ref])? {
        git.capture(["show-ref", "--hash", "--verify", &tag_ref])?
    } else {
        bail!("local tag not found");
    };
    let remote = git.capture(["ls-remote", &args.remote, &tag_ref])?;
    let Some(remote) = remote.split('\t').next().filter(|hash| !hash.is_empty()) else {
        bail!("remote tag not found");
    };
    if local.trim_end_matches('\n') != remote {
        bail!("local tag ref does not match remote tag ref");
    }

    let range = format!("{}/{}..{}", args.remote, args.branch, tag_ref);
    if !git.capture(["rev-list", &range])?.is_empty() {
        bail!("unpushed commits");
    }
    Ok(())
}
