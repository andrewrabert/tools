use std::fs::{self, File};
use std::path::Path;

use anyhow::{Result, bail};
use indicatif::ProgressBar;

use crate::tools::file_hash_recorder::hash::Hashes;
use crate::tools::file_hash_recorder::report::Records;

pub fn run(records: &Records, root: &Path, db_dir: &Path) -> Result<()> {
    println!("Verifying {}", root.display());
    let progress = ProgressBar::new(records.len() as u64);
    let mut failed = false;
    let mut error = |message: String| {
        failed = true;
        progress.suspend(|| tracing::error!("{message}"));
    };
    for (name, record) in records {
        let path = db_dir.join(name);
        match fs::metadata(&path) {
            Err(_) => error(format!("File not found: {}", path.display())),
            Ok(metadata) => {
                if metadata.len() != record.fingerprint.stamp.size {
                    error(format!("size mismatch: {}", path.display()));
                }
                match File::open(&path).and_then(Hashes::of_reader) {
                    Err(hashing) => error(format!("hashing {}: {hashing}", path.display())),
                    Ok(computed) => {
                        let recorded = record.fingerprint.hashes.named();
                        for ((algorithm, actual), (_, expected)) in
                            computed.named().into_iter().zip(recorded)
                        {
                            if actual != expected {
                                error(format!("{algorithm} mismatch: {}", path.display()));
                            }
                        }
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
