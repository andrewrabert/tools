use std::collections::{BTreeSet, HashMap, HashSet};
use std::env;
use std::fmt;
use std::io::{self, IsTerminal, Read};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;

use crate::git::Git;
use crate::tools::Tool;
use crate::tools::git::exit_code;

#[derive(ClapArgs)]
#[command(about = "Restore files deleted from a git repository")]
pub struct Undelete {
    #[arg(
        short = 'C',
        value_name = "PATH",
        help = "run as if git was started in PATH"
    )]
    pub repo: Option<PathBuf>,
    #[arg(short, long, help = "list all deleted files")]
    pub list: bool,
    #[arg(short = '0', long, help = "read null-delimited input from stdin")]
    pub null: bool,
    #[arg(
        short = 'c',
        long,
        help = "show the last commit where the file exists instead of restoring"
    )]
    pub show_commit: bool,
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,
}

impl Tool for Undelete {
    fn run(self) -> ExitCode {
        exit_code(run(self))
    }
}

enum Revision {
    Commit(String),
    ParentOf(String),
}

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Revision::Commit(commit) => write!(f, "{commit}"),
            Revision::ParentOf(commit) => write!(f, "{commit}^"),
        }
    }
}

struct History {
    paths: BTreeSet<String>,
    last: HashMap<String, Revision>,
}

impl History {
    fn read(git: Git) -> Result<Self> {
        let log = git.capture(["log", "--pretty=format:%H", "--name-status"])?;
        let mut history = History {
            paths: BTreeSet::new(),
            last: HashMap::new(),
        };
        let mut commit = None;
        for line in log.lines().filter(|line| !line.is_empty()) {
            let parts: Vec<&str> = line.split('\t').collect();
            let [status, path, rest @ ..] = parts.as_slice() else {
                commit = Some(line);
                continue;
            };
            let Some(commit) = commit else {
                bail!("git log listed {path:?} before any commit");
            };
            let renamed = status.starts_with('R');
            match rest {
                [new_path] if renamed => {
                    history.paths.insert(path.to_string());
                    history.paths.insert(new_path.to_string());
                }
                _ if renamed => {}
                _ => {
                    history.paths.insert(path.to_string());
                }
            }
            let removed =
                (status.starts_with('D') && rest.is_empty()) || (renamed && rest.len() == 1);
            history.last.entry(path.to_string()).or_insert_with(|| {
                if removed {
                    Revision::ParentOf(commit.to_owned())
                } else {
                    Revision::Commit(commit.to_owned())
                }
            });
        }
        Ok(history)
    }

    fn deleted<'a>(
        &'a self,
        root: &'a Path,
        filters: &'a HashSet<PathBuf>,
    ) -> impl Iterator<Item = &'a String> {
        self.paths.iter().filter(move |path| {
            let path = Path::new(path.as_str());
            !root.join(path).exists()
                && (filters.is_empty() || path.ancestors().any(|dir| filters.contains(dir)))
        })
    }
}

fn run(args: Undelete) -> Result<()> {
    let start = match &args.repo {
        Some(dir) => Git::at(dir),
        None => Git::cwd(),
    };
    let root = PathBuf::from(start.toplevel()?);
    let git = Git::at(&root);

    let cwd = env::current_dir().context("locating the current directory")?;
    let mut filters = HashSet::new();
    for path in &args.paths {
        let absolute = normalize(&cwd.join(path));
        let Ok(relative) = absolute.strip_prefix(&root) else {
            bail!("path is outside of the repo root: {}", path.display());
        };
        filters.insert(relative.to_path_buf());
    }

    let history = History::read(git)?;
    if args.list {
        for path in history.deleted(&root, &filters) {
            if !args.show_commit {
                println!("{path}");
            } else if let Some(revision) = history.last.get(path) {
                println!("{revision}\t{path}");
            }
        }
        return Ok(());
    }

    let mut paths: Vec<String> = if !filters.is_empty() {
        history.deleted(&root, &filters).cloned().collect()
    } else if !io::stdin().is_terminal() {
        let mut input = String::new();
        io::stdin()
            .read_to_string(&mut input)
            .context("reading stdin")?;
        if args.null {
            input
                .split('\0')
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .collect()
        } else {
            input.lines().map(str::to_owned).collect()
        }
    } else {
        Vec::new()
    };
    paths.sort();

    for path in &paths {
        let revision = history.last.get(path);
        if args.show_commit {
            if let Some(revision) = revision {
                println!("{path}\t{revision}");
            }
            continue;
        }
        println!("{path}");
        if let Some(revision) = revision {
            git.run(["checkout", &revision.to_string(), "--", path])?;
        }
    }
    Ok(())
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            component => normalized.push(component),
        }
    }
    normalized
}
