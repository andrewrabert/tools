use std::process::ExitCode;

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::git::Git;
use crate::tools::Tool;
use crate::tools::git::exit_code;

#[derive(ClapArgs)]
#[command(about = "List the tags of a remote repository in version order")]
pub struct RemoteTags {
    #[arg(value_name = "URL")]
    pub url: String,
}

impl Tool for RemoteTags {
    fn run(self) -> ExitCode {
        exit_code(run(&self.url))
    }
}

fn run(url: &str) -> Result<()> {
    let refs = Git::cwd().capture([
        "-c",
        "versionsort.suffix=-",
        "ls-remote",
        "--tags",
        "--sort=v:refname",
        url,
    ])?;
    for line in refs.lines() {
        let name = line.split_once('\t').map_or(line, |(_, name)| name);
        println!("{}", name.strip_prefix("refs/tags/").unwrap_or(name));
    }
    Ok(())
}
