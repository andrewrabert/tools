use std::collections::HashSet;
use std::env;
use std::io;
use std::path::{self, Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use signal_hook::consts::SIGINT;

use crate::tools::archive::create::report::SizeReport;
use crate::tools::archive::pool;
use crate::tools::img::cache::{Broker, Cache, Level, Stamp};
use crate::tools::img::cli::Optim;
use crate::tools::img::mime::{self, DEFAULT_OPTIM, Mime};
use crate::tools::img::optimize::{self, Optimized, Options};
use crate::tools::img::process::ProcessFailed;
use crate::tools::img::{cache, jobs, require_file, walk};

const INTERRUPTED: u8 = 130;

struct Candidate {
    path: PathBuf,
    mime: Mime,
    bigtiff: bool,
}

/// A file the path cache does not know, to be looked up by content hash.
struct Unknown {
    candidate: Candidate,
    stamp: Stamp,
}

enum Outcome {
    Done(Result<Optimized>),
    Interrupted,
}

pub fn run(args: Optim) -> Result<ExitCode> {
    let jobs = jobs(args.num_procs)?;
    let hash_jobs = match args.hash_threads {
        Some(threads) => self::jobs(threads)?,
        None => jobs,
    };
    let excludes = exclude_matcher(&args.excludes)?;
    let gitignores = if args.no_ignore {
        Vec::new()
    } else {
        gitignore_matchers(&args.paths)?
    };
    let wanted: HashSet<Mime> = if args.types.is_empty() {
        DEFAULT_OPTIM.into_iter().collect()
    } else {
        args.types.iter().map(|t| t.mime()).collect()
    };
    let level = Level {
        fast: args.fast,
        strip: args.strip,
    };

    let mut cache = if args.no_cache {
        None
    } else {
        Some(open_cache(&args)?)
    };

    // Phase 1: filter, and check the path cache; content hashing is deferred.
    let mut candidates = Vec::new();
    let mut unknown = Vec::new();
    for file in walk::all_files(&args.paths)? {
        if is_excluded(&excludes, &gitignores, &file) {
            continue;
        }
        let detected = match mime::detect(&file) {
            Ok(Some(detected)) => detected,
            Ok(None) => continue,
            // A broken symlink, most likely.
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", file.display()));
            }
        };
        if !wanted.contains(&detected.mime) {
            continue;
        }
        if args.list {
            println!("{}", file.display());
            continue;
        }
        let candidate = Candidate {
            path: file,
            mime: detected.mime,
            bigtiff: detected.bigtiff,
        };
        if let Some(cache) = &cache {
            let stamp = Stamp::of(&candidate.path)?;
            match cache.get(&candidate.path)? {
                Some(record) => {
                    if record.stamp == stamp
                        && record.image.is_type(candidate.mime)
                        && record.image.level.covers(level)
                    {
                        continue;
                    }
                }
                None => {
                    unknown.push(Unknown { candidate, stamp });
                    continue;
                }
            }
        }
        candidates.push(candidate);
    }

    // Phase 2: hash the unknown files concurrently.
    let mut hashed = Vec::with_capacity(unknown.len());
    pool::run(
        unknown,
        hash_jobs,
        |file| optimize::sha256_of(&file.candidate.path),
        |file, sha256| hashed.push((file, sha256)),
    );
    hashed.sort_by(|(a, _), (b, _)| a.candidate.path.cmp(&b.candidate.path));

    // Phase 3: look the hashes up, remembering the path of every known image.
    for (Unknown { candidate, stamp }, sha256) in hashed {
        let sha256 = sha256?;
        if let Some(cache) = &mut cache
            && let Some(image) = cache.get_by_sha256(&sha256)?
            && image.is_type(candidate.mime)
        {
            cache.upsert_path(&candidate.path, &sha256, stamp)?;
            if image.level.covers(level) {
                continue;
            }
        }
        candidates.push(candidate);
    }

    // Phase 4: optimize.
    let candidates: Vec<Candidate> = candidates
        .into_iter()
        .filter(|candidate| optimize::supports(candidate.mime))
        .collect();
    let compute_hash = cache.is_some();
    let interrupted = Arc::new(AtomicBool::new(false));
    let signal = signal_hook::flag::register(SIGINT, Arc::clone(&interrupted))
        .context("installing the SIGINT handler")?;
    let mut report = SizeReport::new(candidates.len());
    let mut failed = false;
    let mut cache_error = None;
    pool::run(
        candidates,
        jobs,
        |candidate| {
            if interrupted.load(Ordering::Relaxed) {
                return Outcome::Interrupted;
            }
            let options = Options {
                fast: args.fast,
                strip: args.strip,
                bigtiff: candidate.bigtiff,
            };
            Outcome::Done(optimize::in_place(
                &candidate.path,
                candidate.mime,
                options,
                compute_hash,
            ))
        },
        |candidate, outcome| match outcome {
            Outcome::Interrupted => {}
            Outcome::Done(Err(error)) => {
                failed = true;
                eprintln!("Error: {}: {error:#}", candidate.path.display());
                if args.verbose
                    && let Some(stderr) = error
                        .downcast_ref::<ProcessFailed>()
                        .and_then(ProcessFailed::stderr)
                {
                    for line in stderr.lines() {
                        eprintln!("Error: {}: {line}", candidate.path.display());
                    }
                }
            }
            Outcome::Done(Ok(optimized)) => {
                if let (Some(cache), Some(sha256)) = (&mut cache, &optimized.sha256)
                    && cache_error.is_none()
                {
                    let recorded = Stamp::of(&candidate.path).and_then(|stamp| {
                        cache.upsert(&candidate.path, stamp, candidate.mime, sha256, level)
                    });
                    if let Err(error) = recorded {
                        cache_error = Some(error);
                    }
                }
                if !args.quiet {
                    report.print(optimized.before, optimized.after, &candidate.path);
                }
            }
        },
    );
    signal_hook::low_level::unregister(signal);

    let interrupted = interrupted.load(Ordering::Relaxed);
    if let Some(cache) = cache {
        cache.close(!interrupted)?;
    }
    if let Some(error) = cache_error {
        return Err(error.context("recording in the cache"));
    }
    if !args.quiet {
        report.print_total();
    }
    Ok(if interrupted {
        ExitCode::from(INTERRUPTED)
    } else if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn open_cache(args: &Optim) -> Result<Broker> {
    let global = cache::global_path()?;
    let cache_path = match &args.cache_path {
        Some(path) => path.clone(),
        None => cache::locate(&args.paths, args.mkdb)?,
    };
    let is_local = path::absolute(&cache_path)? != path::absolute(&global)?;
    let primary = Cache::open(&cache_path, is_local)?;
    let secondary = if is_local && global.is_file() {
        Some(Cache::open(&global, false)?)
    } else {
        None
    };
    let mut hash_dbs = Vec::new();
    for path in &args.hash_dbs {
        require_file(path)?;
        hash_dbs.push(Cache::open_read_only(path)?);
    }
    Broker::new(primary, secondary, hash_dbs)
}

fn exclude_matcher(patterns: &[String]) -> Result<Option<Gitignore>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let cwd = env::current_dir().context("locating the current directory")?;
    let mut builder = GitignoreBuilder::new(cwd);
    for pattern in patterns {
        builder
            .add_line(None, pattern)
            .with_context(|| format!("invalid exclude pattern {pattern:?}"))?;
    }
    Ok(Some(builder.build()?))
}

/// Every .gitignore in or above the inputs, nearest first.
fn gitignore_matchers(paths: &[PathBuf]) -> Result<Vec<Gitignore>> {
    let mut seen = HashSet::new();
    let mut matchers = Vec::new();
    for path in paths {
        let absolute =
            path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        let root = if absolute.is_dir() {
            absolute.as_path()
        } else {
            absolute.parent().unwrap_or(&absolute)
        };
        for dir in root.ancestors() {
            let gitignore = dir.join(".gitignore");
            if !gitignore.is_file() || !seen.insert(gitignore.clone()) {
                continue;
            }
            let mut builder = GitignoreBuilder::new(dir);
            if let Some(error) = builder.add(&gitignore) {
                return Err(error).with_context(|| format!("reading {}", gitignore.display()));
            }
            matchers.push(builder.build()?);
        }
    }
    Ok(matchers)
}

fn is_excluded(excludes: &Option<Gitignore>, gitignores: &[Gitignore], file: &Path) -> bool {
    if let Some(excludes) = excludes
        && excludes
            .matched_path_or_any_parents(file, false)
            .is_ignore()
    {
        return true;
    }
    let absolute = path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    for gitignore in gitignores {
        match gitignore.matched_path_or_any_parents(&absolute, false) {
            Match::None => continue,
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
        }
    }
    false
}
