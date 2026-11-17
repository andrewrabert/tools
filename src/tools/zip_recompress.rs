use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::pool;
use crate::tools::Tool;
use crate::tools::archive::create::report::SizeReport;
use crate::zip_recompress;

#[derive(ClapArgs)]
#[command(about = "Recompress zip archives in place with zopfli (advzip --shrink-insane)")]
pub struct ZipRecompress {
    #[arg(
        short = 'n',
        long = "num-concurrent",
        value_name = "NUM",
        default_value_t = 0,
        help = "number of entries compressed in parallel, 0 for cpu count"
    )]
    num_procs: usize,
    #[arg(value_name = "FILE", required = true)]
    files: Vec<PathBuf>,
}

impl Tool for ZipRecompress {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: ZipRecompress) -> Result<ExitCode> {
    let workers = pool::jobs(args.num_procs)?;
    let mut report = SizeReport::new(args.files.len());
    let mut code = ExitCode::SUCCESS;
    for file in &args.files {
        match zip_recompress::recompress(file, workers) {
            Ok(outcome) => report.print(outcome.before, outcome.after, file),
            Err(error) => {
                eprintln!("Error: {error:?}");
                code = ExitCode::FAILURE;
            }
        }
    }
    report.print_total();
    Ok(code)
}
