use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use anyhow::{Context, Result};
use indicatif::ProgressBar;

use crate::pool;
use crate::tools::file_hash_recorder::archive;
use crate::tools::file_hash_recorder::config::{
    ArchivePolicy, Feedback, Naming, OnError, Rehash, UpdateOptions,
};
use crate::tools::file_hash_recorder::db::FileInfoDb;
use crate::tools::file_hash_recorder::record::{ArchiveContents, ContentKey, FileRecord, Stamp};
use crate::tools::file_hash_recorder::report::Records;
use crate::tools::file_hash_recorder::scan;
use crate::tools::file_hash_recorder::walk;

struct Pending {
    path: PathBuf,
    name: String,
    stamp: Stamp,
}

type ArchiveCache = Mutex<HashMap<ContentKey, ArchiveContents>>;

pub fn run(
    db: &mut FileInfoDb,
    root: &Path,
    recorded: Records,
    options: &UpdateOptions,
    naming: Naming,
    feedback: Feedback,
) -> Result<()> {
    let mut found = BTreeSet::new();
    let mut pending = Vec::new();
    for path in walk::files_under(root)? {
        if path == db.path() {
            continue;
        }
        let name = match naming {
            Naming::Absolute => scan::utf8_name(&path)?,
            Naming::Relative => scan::relative_name(&path, db.dir())?,
        };
        let metadata = fs::metadata(&path).with_context(|| format!("stat {}", path.display()))?;
        let stamp = Stamp::from(&metadata);
        found.insert(name.clone());
        if !is_current(recorded.get(&name), &path, stamp, options) {
            pending.push(Pending { path, name, stamp });
        }
    }

    let cache: ArchiveCache = Mutex::new(
        recorded
            .values()
            .map(|record| {
                (
                    record.fingerprint.content_key(),
                    record.archive_contents.clone(),
                )
            })
            .collect(),
    );

    let progress = match feedback {
        Feedback::ProgressBar => {
            println!("Processing {}", root.display());
            ProgressBar::new(pending.len() as u64)
        }
        Feedback::EachFile | Feedback::Silent => ProgressBar::hidden(),
    };

    pool::run(
        pending,
        options.jobs,
        |pending| {
            if let Feedback::EachFile = feedback {
                println!("Processing {}", pending.path.display());
            }
            process(pending, options.archives, &cache)
        },
        |pending, record| {
            progress.inc(1);
            match (record, options.on_error) {
                (Ok(record), _) => db.upsert(&pending.name, &record)?,
                (Err(error), on_error) => {
                    progress.suspend(|| {
                        eprintln!("error processing \"{}\": {error:#}", pending.path.display());
                    });
                    if let OnError::Abort = on_error {
                        return Err(error);
                    }
                }
            }
            Ok(())
        },
    )?;
    progress.finish_and_clear();

    for name in recorded.keys().filter(|name| !found.contains(*name)) {
        db.delete(name)?;
    }
    Ok(())
}

fn is_current(
    recorded: Option<&FileRecord>,
    path: &Path,
    stamp: Stamp,
    options: &UpdateOptions,
) -> bool {
    let Some(recorded) = recorded else {
        return false;
    };
    if let Rehash::Everything = options.rehash {
        return false;
    }
    if recorded.fingerprint.stamp != stamp {
        return false;
    }
    match options.archives {
        ArchivePolicy::Record(_) if archive::has_archive_suffix(path) => {
            !recorded.archive_contents.is_empty()
        }
        ArchivePolicy::Record(_) | ArchivePolicy::Ignore => true,
    }
}

fn process(pending: &Pending, archives: ArchivePolicy, cache: &ArchiveCache) -> Result<FileRecord> {
    let fingerprint = scan::fingerprint_as_of(&pending.path, pending.stamp)?;
    let archive_contents = match archives {
        ArchivePolicy::Ignore => ArchiveContents::new(),
        ArchivePolicy::Record(extraction) => {
            let key = fingerprint.content_key();
            let known = cache
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&key)
                .cloned()
                .unwrap_or_default();
            let contents = if known.is_empty() && archive::has_archive_suffix(&pending.path) {
                archive::contents(&pending.path, extraction)?
            } else {
                known
            };
            cache
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key, contents.clone());
            contents
        }
    };
    Ok(FileRecord {
        fingerprint,
        archive_contents,
    })
}
