mod cli;
mod scan;

use std::fs;
use std::io;
use std::path::Path;
use std::process::ExitCode;

use crate::tools::Tool;
pub use crate::tools::empty_tree::cli::EmptyTree;

impl Tool for EmptyTree {
    fn run(self) -> ExitCode {
        let mut roots = self.paths;
        roots.sort();
        roots.dedup();

        for root in roots {
            if !root.exists() {
                continue;
            }
            let mut empties = scan::empty_dirs(&root, self.parent);
            empties.sort();
            for dir in empties.iter().rev() {
                if !self.quiet {
                    println!("{}", dir.display());
                }
                if self.remove {
                    remove(dir);
                }
            }
        }
        ExitCode::SUCCESS
    }
}

fn remove(dir: &Path) {
    match fs::remove_dir(dir) {
        Ok(()) => {}
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            ) => {}
        Err(e) => eprintln!("{}: {e}", dir.display()),
    }
}
