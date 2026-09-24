use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args as ClapArgs;
use walkdir::WalkDir;

use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "List every git repository under a directory")]
pub struct FindRepos {
    #[arg(value_name = "DIR", default_value = ".")]
    pub dir: PathBuf,
}

impl Tool for FindRepos {
    fn run(self) -> ExitCode {
        let mut code = ExitCode::SUCCESS;
        let mut walk = WalkDir::new(&self.dir).sort_by_file_name().into_iter();
        while let Some(entry) = walk.next() {
            match entry {
                Ok(entry) if entry.file_type().is_dir() && entry.file_name() == ".git" => {
                    walk.skip_current_dir();
                    let repo = entry
                        .path()
                        .parent()
                        .filter(|repo| !repo.as_os_str().is_empty());
                    println!("{}", repo.unwrap_or(Path::new(".")).display());
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("Error: {error}");
                    code = ExitCode::FAILURE;
                }
            }
        }
        code
    }
}
