use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::num::ParseIntError;
use std::path::{self, Path, PathBuf};
use std::process::{Command, ExitCode};
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use reqwest::Url;

use crate::dirs;
use crate::git::{self, Git};
use crate::tools::Tool;
use crate::tools::git::exit_code;

const REMOTES: [&str; 2] = ["upstream", "origin"];

#[derive(ClapArgs)]
#[command(about = "Print and open the web page of a repository or a file in it")]
pub struct Web {
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,
    #[arg(short, long, value_name = "START[-END]", help = "lines to link to")]
    pub lines: Option<Lines>,
}

impl Tool for Web {
    fn run(self) -> ExitCode {
        exit_code(run(self))
    }
}

#[derive(Clone, Copy)]
pub struct Lines {
    start: u64,
    end: Option<u64>,
}

impl FromStr for Lines {
    type Err = ParseIntError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value.split_once('-') {
            Some((start, end)) => Lines {
                start: start.parse()?,
                end: Some(end.parse()?),
            },
            None => Lines {
                start: value.parse()?,
                end: None,
            },
        })
    }
}

enum RemoteUrl {
    Web(String),
    Scp { host: String, path: String },
}

impl FromStr for RemoteUrl {
    type Err = anyhow::Error;

    fn from_str(url: &str) -> Result<Self> {
        let lower = url.to_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return Ok(RemoteUrl::Web(url.to_owned()));
        }
        let Some((host, path)) = url.split_once(':') else {
            bail!("unsupported remote url: {url}");
        };
        Ok(RemoteUrl::Scp {
            host: host.to_owned(),
            path: path.to_owned(),
        })
    }
}

impl RemoteUrl {
    fn web(self, ssh_hostnames: &HashMap<String, String>) -> String {
        let (host, path) = match self {
            RemoteUrl::Web(url) => return url,
            RemoteUrl::Scp { host, path } => (host, path),
        };
        let hostname = match ssh_hostnames.get(&host) {
            Some(hostname) => hostname.as_str(),
            None => host.split_once('@').map_or(host.as_str(), |(_, host)| host),
        };
        if hostname == "aur.archlinux.org" {
            format!("https://aur.archlinux.org/packages/{path}")
        } else {
            format!("https://{hostname}/{path}")
        }
    }
}

fn run(args: Web) -> Result<()> {
    let path = match args.path {
        Some(path) => {
            path::absolute(&path).with_context(|| format!("resolving {}", path.display()))?
        }
        None => PathBuf::from(Git::cwd().toplevel()?),
    };
    let dir = if path.is_file() {
        path.parent().unwrap_or(&path)
    } else {
        path.as_path()
    };
    let git = Git::at(dir);

    let (remote, remote_url) = remote(git)?;
    let base = remote_url.parse::<RemoteUrl>()?.web(&ssh_hostnames()?);

    let toplevel = PathBuf::from(git.toplevel()?);
    let relative = path
        .strip_prefix(&toplevel)
        .with_context(|| format!("{} is outside of {}", path.display(), toplevel.display()))?;
    let url = if relative.as_os_str().is_empty() && args.lines.is_none() {
        base
    } else {
        let branch = preferred_branch(git, remote)?;
        blob_url(base, &branch, relative, args.lines)?
    };

    println!("{url}");
    let over_ssh = env::var_os("SSH_TTY").is_some_and(|tty| !tty.is_empty());
    if io::stdout().is_terminal() && !over_ssh {
        git::run(Command::new("open").arg(&url))?;
    }
    Ok(())
}

fn remote(git: Git) -> Result<(&'static str, String)> {
    for remote in REMOTES {
        if git.succeeds(["remote", "get-url", remote])? {
            let url = git.capture(["remote", "get-url", remote])?;
            return Ok((remote, url.trim().to_owned()));
        }
    }
    bail!("no remote named {}", REMOTES.join(" or "));
}

fn preferred_branch(git: Git, remote: &str) -> Result<String> {
    let current = git.capture(["branch", "--show-current"])?;
    for branch in [current.trim(), "main", "master"] {
        if branch.is_empty() {
            continue;
        }
        let tracking = format!("refs/remotes/{remote}/{branch}");
        if git.succeeds(["rev-parse", "--verify", "--quiet", &tracking])? {
            return Ok(branch.to_owned());
        }
    }
    bail!("{remote} has none of the current branch, main, or master");
}

fn blob_url(base: String, branch: &str, relative: &Path, lines: Option<Lines>) -> Result<String> {
    let parsed = Url::parse(&base).with_context(|| format!("parsing {base}"))?;
    let host = parsed.host_str().unwrap_or_default().to_lowercase();
    let mut url = base;
    let (blob, end_prefix) = if host.contains("github") {
        ("blob", "L")
    } else if host.contains("gitlab") {
        ("-/blob", "")
    } else {
        eprintln!("error: unhandled git host");
        return Ok(url);
    };
    url.push_str(&format!("/{blob}/{branch}"));
    for component in relative.components() {
        url.push('/');
        url.push_str(&component.as_os_str().to_string_lossy());
    }
    if let Some(Lines { start, end }) = lines.filter(|lines| lines.start != 0) {
        url.push_str(&format!("#L{start}"));
        if let Some(end) = end {
            url.push_str(&format!("-{end_prefix}{end}"));
        }
    }
    Ok(url)
}

fn ssh_hostnames() -> Result<HashMap<String, String>> {
    let home = dirs::home()?;
    let config_path = home.join(".ssh/config");
    let config = match fs::read_to_string(&config_path) {
        Ok(config) => config,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", config_path.display()));
        }
    };
    let mut hostnames = HashMap::new();
    let mut host = None;
    for line in config.lines() {
        let Some((key, value)) = line.trim().split_once(char::is_whitespace) else {
            continue;
        };
        let value = value.trim();
        if key.eq_ignore_ascii_case("host") {
            host = Some(value);
        } else if key.eq_ignore_ascii_case("hostname")
            && let Some(host) = host
        {
            hostnames.insert(host.to_owned(), value.to_owned());
        }
    }
    Ok(hostnames)
}
