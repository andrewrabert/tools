use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;

use crate::tools::file_hash_recorder::hash::Hashes;
use crate::tools::file_hash_recorder::report::Records;

pub fn run(records: &Records, root: &Path, db_dir: &Path) -> Result<()> {
    println!("Verifying {}", root.display());
    let progress = ProgressBar::new(records.len() as u64);
    let mut failed = false;
    for (name, record) in records {
        let path = db_dir.join(name);
        let mut report = |problem: &str| {
            failed = true;
            progress.suspend(|| eprintln!("{problem}: {}", path.display()));
        };
        match fs::metadata(&path) {
            Err(_) => report("File not found"),
            Ok(metadata) => {
                if metadata.len() != record.fingerprint.stamp.size {
                    report("size mismatch");
                }
                let computed = Hashes::of_file(&path)
                    .with_context(|| format!("hashing {}", path.display()))?;
                let recorded = record.fingerprint.hashes.named();
                for ((algorithm, actual), (_, expected)) in
                    computed.named().into_iter().zip(recorded)
                {
                    if actual != expected {
                        report(&format!("{algorithm} mismatch"));
                    }
                }
            }
        }
        progress.inc(1);
    }
    progress.finish_and_clear();
    if failed {
        bail!("verification failed");
    }
    Ok(())
}
