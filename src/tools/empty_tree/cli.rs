use std::path::PathBuf;

use clap::Args as ClapArgs;

#[derive(ClapArgs)]
#[command(about = "List the tops of empty directory trees")]
pub struct EmptyTree {
    #[arg(short, long, help = "allow the parent to be removed if empty")]
    pub parent: bool,
    #[arg(short, long, help = "remove the trees from disk")]
    pub remove: bool,
    #[arg(short, long, help = "suppress non-error output")]
    pub quiet: bool,
    #[arg(value_name = "PATH", default_value = ".")]
    pub paths: Vec<PathBuf>,
}
