mod archive;
mod cli;
mod config;
mod db;
mod hash;
mod locate;
mod record;
mod report;
pub(crate) mod scan;
mod update;
mod verify;
pub(crate) mod walk;

use std::path::{self, Path};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

use crate::tools::Tool;
pub use crate::tools::file_hash_recorder::cli::FileHashRecorder;
use crate::tools::file_hash_recorder::config::{Action, Config, Database, Naming};
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
        recorded(root, &config)?;
    }
    Ok(())
}

fn recorded(root: &Path, config: &Config) -> Result<()> {
    let root = path::absolute(root).with_context(|| format!("resolving {}", root.display()))?;
    let db_path = match &config.database {
        Database::Given(path) => {
            path::absolute(path).with_context(|| format!("resolving {}", path.display()))?
        }
        Database::Discovered(missing) => locate::database_for(&root, *missing)?,
    };
    FileInfoDb::with(&db_path, |db| {
        let root = if root == db_path {
            db.dir().to_path_buf()
        } else {
            root
        };

        let scope = scope_of(&root, db.dir())?;
        let records = db.records(&scope)?;
        match &config.action {
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
                update::run(db, &root, records, options, config.naming, config.feedback)?;
            }
        }
        Ok(())
    })
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
