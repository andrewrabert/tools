mod cache;
mod clean;
mod cli;
mod discover;
mod source;

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::dirs;
use crate::git::{self, Git};
use crate::tools::Tool;
use crate::tools::git::exit_code;
use crate::tools::git::sync::cache::RepoList;
pub use crate::tools::git::sync::cli::Sync;
use crate::tools::git::sync::source::{Forge, Source};

impl Tool for Sync {
    fn run(self) -> ExitCode {
        exit_code(run(self))
    }
}

enum Action {
    ListHosts,
    ParseUrl(String),
    BuildCache,
    Tracked(Tracked),
}

enum Tracked {
    ShowCache,
    ShowBackupCache,
    CleanEmpty { dry_run: bool },
    FetchAll,
    Remove(String),
    Sync(String),
}

impl Action {
    fn from_args(args: Sync) -> Result<Action> {
        if args.list_hosts {
            return Ok(Action::ListHosts);
        }
        if args.parse_url {
            let url = args.repo.context("--parse-url requires a URL argument")?;
            return Ok(Action::ParseUrl(url));
        }
        let tracked = if args.show_cache {
            Tracked::ShowCache
        } else if args.show_backup_cache {
            Tracked::ShowBackupCache
        } else if args.clean_empty {
            Tracked::CleanEmpty {
                dry_run: args.dry_run,
            }
        } else if args.build_cache {
            return Ok(Action::BuildCache);
        } else {
            match args.repo {
                None => Tracked::FetchAll,
                Some(repo) if args.rm => Tracked::Remove(repo),
                Some(repo) => Tracked::Sync(repo),
            }
        };
        Ok(Action::Tracked(tracked))
    }
}

struct Dirs {
    root: PathBuf,
    cache: PathBuf,
    backup_cache: PathBuf,
}

impl Dirs {
    fn locate() -> Result<Dirs> {
        let cache_dir = dirs::cache()?;
        Ok(Dirs {
            root: dirs::home()?.join("src"),
            cache: cache_dir.join("git-sync"),
            backup_cache: cache_dir.join("git-sync-backup"),
        })
    }
}

struct Target {
    entry: String,
    dir: PathBuf,
    source: Source,
    backup: bool,
}

impl Target {
    fn resolve(repo: &str, root: &Path) -> Result<Target> {
        let backup = repo.starts_with("backup/");
        let direct = root.join(repo);
        if direct.exists() {
            return Ok(Target {
                entry: repo.to_owned(),
                dir: direct,
                source: repo.parse()?,
                backup,
            });
        }
        let source: Source = repo.parse()?;
        Ok(Target {
            entry: repo.to_lowercase(),
            dir: source.dir(root),
            source,
            backup,
        })
    }
}

fn run(args: Sync) -> Result<()> {
    match Action::from_args(args)? {
        Action::ListHosts => {
            for forge in Forge::ALL {
                println!("{}", forge.name());
            }
            Ok(())
        }
        Action::ParseUrl(url) => {
            println!("{}", url.parse::<Source>()?);
            Ok(())
        }
        Action::BuildCache => with_root(build_cache),
        Action::Tracked(tracked) => with_root(|dirs| run_tracked(tracked, dirs)),
    }
}

fn with_root(run: impl FnOnce(&Dirs) -> Result<()>) -> Result<()> {
    let dirs = Dirs::locate()?;
    if !dirs.root.is_dir() {
        return Ok(());
    }
    run(&dirs)
}

fn run_tracked(tracked: Tracked, dirs: &Dirs) -> Result<()> {
    let mut cache = RepoList::load(dirs.cache.clone(), &dirs.root)?;
    let mut backup_cache = RepoList::load(dirs.backup_cache.clone(), &dirs.root)?;
    match tracked {
        Tracked::ShowCache => println!("{}", cache.entries().join("\n")),
        Tracked::ShowBackupCache => println!("{}", backup_cache.entries().join("\n")),
        Tracked::CleanEmpty { dry_run } => {
            let deleted = clean::empty_dirs(&dirs.root, dry_run);
            println!("Deleted {deleted} empty directories");
        }
        Tracked::FetchAll => fetch_all()?,
        Tracked::Remove(repo) => {
            let target = Target::resolve(&repo, &dirs.root)?;
            if target.dir.exists() {
                fs::remove_dir_all(&target.dir)
                    .with_context(|| format!("deleting {}", target.dir.display()))?;
            }
            let list = if target.backup {
                &mut backup_cache
            } else {
                &mut cache
            };
            list.remove(&target.entry);
        }
        Tracked::Sync(repo) => {
            let target = Target::resolve(&repo, &dirs.root)?;
            let list = if target.backup {
                &mut backup_cache
            } else {
                &mut cache
            };
            sync(target, list)?;
        }
    }
    cache.save()?;
    backup_cache.save()
}

fn sync(target: Target, list: &mut RepoList) -> Result<()> {
    let Target {
        mut entry,
        dir,
        source,
        ..
    } = target;
    if !dir.exists() {
        entry = source.clone_url();
        git::run(Git::cwd().command(["clone"]).arg(&entry).arg(&dir))?;
    }
    list.promote(entry);
    println!("{}", dir.display());
    Ok(())
}

fn fetch_all() -> Result<()> {
    let cwd = env::current_dir().context("locating the current directory")?;
    for repo in discover::repos(&cwd) {
        println!("{}", repo.display());
        Git::at(&repo).run(["fetch", "--all"])?;
    }
    Ok(())
}

fn build_cache(dirs: &Dirs) -> Result<()> {
    let mut regular = Vec::new();
    let mut backup = Vec::new();
    for repo in discover::repos(&dirs.root) {
        let entry = cache_entry(&repo);
        if repo.to_string_lossy().starts_with("backup/") {
            backup.push(entry);
        } else {
            regular.push(entry);
        }
    }
    let regular = keep_order(cache::read_lines(&dirs.cache)?, regular);
    let backup = keep_order(cache::read_lines(&dirs.backup_cache)?, backup);
    cache::write_lines(&dirs.cache, &regular)?;
    cache::write_lines(&dirs.backup_cache, &backup)?;
    println!("Built cache with {} regular repositories", regular.len());
    println!(
        "Built backup cache with {} backup repositories",
        backup.len()
    );
    Ok(())
}

fn cache_entry(repo: &Path) -> String {
    let mut components = repo.components();
    let forge = components
        .next()
        .and_then(|first| first.as_os_str().to_str())
        .and_then(Forge::from_name);
    match forge {
        Some(forge) => format!("{}:{}", forge.name(), components.as_path().display()),
        None => repo.display().to_string(),
    }
}

fn keep_order(cached: Vec<String>, found: Vec<String>) -> Vec<String> {
    let cached: Vec<String> = cached
        .into_iter()
        .filter(|entry| !entry.is_empty())
        .collect();
    let known: HashSet<&String> = cached.iter().collect();
    let found_set: HashSet<&String> = found.iter().collect();
    let mut new: Vec<String> = found
        .iter()
        .filter(|entry| !known.contains(entry))
        .cloned()
        .collect();
    new.sort();
    let mut ordered: Vec<String> = cached
        .iter()
        .filter(|entry| found_set.contains(entry))
        .cloned()
        .collect();
    ordered.append(&mut new);
    ordered
}
