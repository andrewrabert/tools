mod archive;
mod cli;
mod config;
mod db;
mod hash;
mod locate;
mod record;
mod report;
mod scan;
mod update;
mod verify;
mod walk;

use std::collections::BTreeMap;
use std::path::{self, Path};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

use crate::tools::Tool;
pub use crate::tools::file_hash_recorder::cli::FileHashRecorder;
use crate::tools::file_hash_recorder::config::{Action, Config, Database, Feedback, Mode, Naming};
use crate::tools::file_hash_recorder::db::{FileInfoDb, Scope};

impl Tool for FileHashRecorder {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: FileHashRecorder) -> Result<()> {
    let config = Config::try_from(args)?;
    for root in &config.roots {
        match &config.mode {
            Mode::Multihash => multihash(root, &config)?,
            Mode::Recorded { database, action } => recorded(root, database, action, &config)?,
        }
    }
    Ok(())
}

fn multihash(root: &Path, config: &Config) -> Result<()> {
    if root.is_file() {
        announce(root, config.feedback);
        return report::print_json(&scan::fingerprint(root)?, config.json);
    }
    let mut fingerprints = BTreeMap::new();
    for path in walk::files_under(root)? {
        announce(&path, config.feedback);
        let name = scan::relative_name(&path, root)?;
        fingerprints.insert(name, scan::fingerprint(&path)?);
    }
    report::print_json(&fingerprints, config.json)
}

fn announce(path: &Path, feedback: Feedback) {
    if let Feedback::EachFile = feedback {
        println!("Processing {}", path.display());
    }
}

fn recorded(root: &Path, database: &Database, action: &Action, config: &Config) -> Result<()> {
    let root = path::absolute(root).with_context(|| format!("resolving {}", root.display()))?;
    let db_path = match database {
        Database::Given(path) => {
            path::absolute(path).with_context(|| format!("resolving {}", path.display()))?
        }
        Database::Discovered(missing) => locate::database_for(&root, *missing)?,
    };
    let mut db = FileInfoDb::open(&db_path)?;
    let root = if root == db_path {
        db.dir().to_path_buf()
    } else {
        root
    };

    let scope = scope_of(&root, db.dir())?;
    let records = db.records(&scope)?;
    match action {
        Action::Size => report::size(&records, &root, config.naming)?,
        Action::List => {
            let listing = report::list(records, &scope, db.dir(), config.naming);
            report::print_json(&listing, config.json)?;
        }
        Action::Dupes => {
            let dupes = report::dupes(records, &scope, db.dir(), config.naming);
            report::print_json(&dupes, config.json)?;
        }
        Action::Verify => verify::run(&records, &root, db.dir())?,
        Action::Update(options) => {
            if let (Scope::Outside, Naming::Relative) = (&scope, config.naming) {
                bail!(
                    "{} is not under the database directory {}",
                    root.display(),
                    db.dir().display()
                );
            }
            update::run(
                &mut db,
                &root,
                records,
                options,
                config.naming,
                config.feedback,
            )?;
        }
    }
    db.close()
}

fn scope_of(root: &Path, db_dir: &Path) -> Result<Scope> {
    if root == db_dir {
        return Ok(Scope::All);
    }
    let Ok(relative) = root.strip_prefix(db_dir) else {
        return Ok(Scope::Outside);
    };
    let relative = scan::utf8_name(relative)?;
    Ok(if root.is_dir() {
        Scope::Under(format!("{relative}/"))
    } else {
        Scope::File(relative)
    })
}
