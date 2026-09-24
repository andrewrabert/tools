use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::tools::reencode_lossless::codec::Target;

#[derive(ClapArgs)]
#[command(about = "Reencode lossless audio files as FLAC or WAV")]
pub struct ReencodeLossless {
    #[arg(
        short = 'n',
        long = "num-concurrent",
        value_name = "NUM",
        default_value_t = 0,
        help = "number of parallel jobs, 0 for cpu count"
    )]
    pub num_procs: usize,
    #[arg(long, value_enum, default_value_t = Target::Flac)]
    pub format: Target,
    #[arg(long)]
    pub flac_decode_through_errors: bool,
    #[arg(value_name = "PATH", required = true)]
    pub paths: Vec<PathBuf>,
}
