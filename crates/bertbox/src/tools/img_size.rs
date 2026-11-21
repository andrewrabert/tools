use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::tools::Tool;
use crate::walk;

const EXTENSIONS: [&str; 8] = ["bmp", "gif", "jpeg", "jpg", "png", "tif", "tiff", "webp"];

#[derive(ClapArgs)]
#[command(about = "Print the width and height of images")]
pub struct ImgSize {
    /// Print only the largest width and height across all images, as WIDTHxHEIGHT
    #[arg(long)]
    max: bool,
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
}

impl Tool for ImgSize {
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

fn run(args: ImgSize) -> Result<()> {
    let mut max = (0, 0);
    for path in images(&args.paths)? {
        let size = imagesize::size(&path).with_context(|| format!("reading {}", path.display()))?;
        if args.max {
            max = (max.0.max(size.width), max.1.max(size.height));
        } else {
            println!("{} {} {}", size.width, size.height, path.display());
        }
    }
    if args.max {
        println!("{}x{}", max.0, max.1);
    }
    Ok(())
}

/// The image files among and under `paths`, sorted.
fn images(paths: &[PathBuf]) -> Result<BTreeSet<PathBuf>> {
    let mut images = BTreeSet::new();
    for path in paths {
        for entry in walk::entries(path) {
            let entry = entry?;
            if entry.file_type().is_file() && is_image(entry.path()) {
                images.insert(entry.into_path());
            }
        }
    }
    Ok(images)
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}
