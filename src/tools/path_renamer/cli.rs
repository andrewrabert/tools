use clap::Args as ClapArgs;

use crate::source::Source;

#[derive(ClapArgs)]
#[command(
    about = "Rename sibling paths by editing their names in nvim; paths are read one per line"
)]
pub struct PathRenamer {
    #[arg(long, help = "suffix colliding targets with _ instead of failing")]
    pub allow_duplicates: bool,
    #[arg(value_name = "DATA", default_value = "-")]
    pub data: Source,
}
