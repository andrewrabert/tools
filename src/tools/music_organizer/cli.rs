use std::path::PathBuf;

use clap::Args as ClapArgs;

#[derive(ClapArgs)]
#[command(about = "Organize a music library by its metadata")]
pub struct MusicOrganizer {
    #[arg(value_name = "DIRECTORY")]
    pub directory: PathBuf,
    #[arg(long, help = "check top-level folders against the library layout")]
    pub library: bool,
    #[arg(short = 't', long, help = "check and fix tags")]
    pub check_tags: bool,
    #[arg(
        long,
        value_name = "DIR",
        help = "enable directory organization; parent directory where artist folders are located"
    )]
    pub organize_dirs: Option<PathBuf>,
    #[arg(long, help = "show what would be done without making changes")]
    pub dry_run: bool,
}
