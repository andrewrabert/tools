use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use ratatui::text::Line;

use crate::dirs;
use crate::dispatch;
use crate::picker::{self, Matching};
use crate::tmux;
use crate::tools::Tool;
use crate::tools::tmux::attach_ssh::{self, Placement, Shell};

const HISTORY_FILE: &str = "tmux-ssh-history";

#[derive(ClapArgs)]
#[command(about = "Pick an SSH host and attach to its tmux session in this window")]
pub struct Ssh {
    #[arg(long, help = "open the session in a new tmux window, as from a popup")]
    pub popup: bool,
}

impl Tool for Ssh {
    fn run(self) -> ExitCode {
        run(self).unwrap_or_else(|error| {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        })
    }
}

struct History {
    path: PathBuf,
    hosts: Vec<String>,
}

impl History {
    fn load() -> Result<Self> {
        let path = dirs::cache()?.join(HISTORY_FILE);
        let hosts = match fs::read_to_string(&path) {
            Ok(text) => text.lines().map(str::to_owned).collect(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", path.display()));
            }
        };
        Ok(History { path, hosts })
    }

    fn contains(&self, host: &str) -> bool {
        self.hosts.iter().any(|known| known == host)
    }

    fn add(&self, host: &str) -> Result<()> {
        if self.contains(host) {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("opening {}", self.path.display()))?;
        writeln!(file, "{host}").with_context(|| format!("writing {}", self.path.display()))
    }
}

fn run(args: Ssh) -> Result<ExitCode> {
    let history = History::load()?;
    let mut hosts = history.hosts.clone();
    hosts.sort();
    let Some(host) = choose(&hosts)? else {
        return Ok(ExitCode::from(130));
    };

    if !history.contains(&host) && !reachable(&host)? {
        eprintln!("error: failed to connect to {host}");
        println!("Press Enter to continue...");
        io::stdin()
            .read_line(&mut String::new())
            .context("reading stdin")?;
        return Ok(ExitCode::FAILURE);
    }
    history.add(&host)?;

    if args.popup {
        let error = tmux::command(["new-window", "-e"])
            .arg(format!("{}={}", dispatch::ENV_NAME, dispatch::ENV_VALUE))
            .arg(dispatch::program()?)
            .args(["tmux-attach-ssh", "--current-window", &host])
            .exec();
        return Err(error).context("running tmux");
    }
    attach_ssh::connect(&host, &Shell::resolve(None)?, Placement::CurrentWindow)?;
    Ok(ExitCode::SUCCESS)
}

fn reachable(host: &str) -> Result<bool> {
    let status = Command::new("ssh")
        .args(["-q", "-o", "BatchMode=yes", "-o", "ConnectTimeout=3", host])
        .args(["--", "zsh", "-lic", "command -v tmux-attach"])
        .status()
        .context("running ssh")?;
    Ok(status.success())
}

/// The host picked or typed, or `None` when the picker was cancelled or left empty.
fn choose(hosts: &[String]) -> Result<Option<String>> {
    let lines: Vec<Line> = hosts.iter().map(|host| Line::raw(host.as_str())).collect();
    let Some(picked) = picker::pick(&lines, "SSH Host: ", Matching::Fuzzy)? else {
        return Ok(None);
    };
    Ok(match picked.item {
        Some(index) => Some(hosts[index].clone()),
        None => Some(picked.query.trim().to_owned()).filter(|host| !host.is_empty()),
    })
}
