use std::num::NonZeroU32;
use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand, ValueEnum};

use crate::tools::img::mime::Mime;

#[derive(ClapArgs)]
#[command(about = "Print a hash of each image's decoded pixels")]
pub struct Compare {
    #[arg(value_name = "PATH", num_args = 2, required = true)]
    pub paths: Vec<PathBuf>,
}

#[derive(ClapArgs)]
#[command(about = "Convert or resize images")]
pub struct Convert {
    #[arg(
        short = 'n',
        long = "num-concurrent",
        value_name = "NUM",
        default_value_t = 0,
        help = "number of parallel jobs, 0 for cpu count"
    )]
    pub num_procs: usize,
    #[arg(long, help = "remove each source once all of its outputs are written")]
    pub rm: bool,
    #[arg(long, help = "overwrite even if destination is the source file")]
    pub force: bool,
    #[arg(
        short,
        long,
        value_name = "PATH",
        help = "output directory or file path"
    )]
    pub output: Option<PathBuf>,
    #[arg(short, long, value_enum)]
    pub format: Option<Format>,
    #[arg(long)]
    pub fast: bool,
    #[arg(
        long,
        help = "merge all inputs (and every page of a multi-page input) into a single \
                multi-page output file; requires -f and -o FILENAME"
    )]
    pub combine: bool,
    #[arg(value_name = "PATH", required = true)]
    pub paths: Vec<PathBuf>,
    #[arg(
        long,
        value_name = "[0-100]",
        default_value_t = 90,
        value_parser = clap::value_parser!(u8).range(0..=100),
        help_heading = "JPEG Options"
    )]
    pub jpeg_quality: u8,
    #[arg(long, help_heading = "JXL Options")]
    pub jxl_lossy: bool,
    #[arg(
        short = 'W',
        long,
        value_name = "PIXELS",
        help = "output width in pixels",
        help_heading = "Resize Options"
    )]
    pub width: Option<NonZeroU32>,
    #[arg(
        short = 'H',
        long,
        value_name = "PIXELS",
        help = "output height in pixels",
        help_heading = "Resize Options"
    )]
    pub height: Option<NonZeroU32>,
    #[arg(
        short = 'S',
        long,
        value_name = "FACTOR",
        value_parser = parse_scale,
        help = "scale factor (e.g. 2, 50%, 1/2)",
        help_heading = "Resize Options"
    )]
    pub scale: Option<f64>,
    #[arg(
        short = 'K',
        long,
        value_enum,
        help = "resampling filter (default: lanczos3)",
        help_heading = "Resize Options"
    )]
    pub filter: Option<Kernel>,
    #[arg(
        long,
        value_enum,
        default_value_t = ResizeMode::Shrink,
        help = "shrink=only downscale, grow=only upscale, stretch=exact WxH",
        help_heading = "Resize Options"
    )]
    pub resize_mode: ResizeMode,
}

#[derive(ClapArgs)]
#[command(about = "Recode PNGs whose pixels are gray but stored as color")]
pub struct Fixcolorspace {
    #[arg(value_name = "PATH", required = true)]
    pub paths: Vec<PathBuf>,
}

#[derive(ClapArgs)]
#[command(about = "Copy metadata between images")]
pub struct Metadata {
    #[command(subcommand)]
    pub command: MetadataCommand,
}

#[derive(Subcommand)]
pub enum MetadataCommand {
    #[command(about = "Copy all metadata from SOURCE to TARGET")]
    Copy {
        #[arg(value_name = "SOURCE")]
        source: PathBuf,
        #[arg(value_name = "TARGET")]
        target: PathBuf,
    },
}

#[derive(ClapArgs)]
#[command(about = "Losslessly shrink images in place")]
pub struct Optim {
    #[arg(short, long)]
    pub strip: bool,
    #[arg(
        short = 't',
        long = "type",
        value_name = "TYPE",
        value_enum,
        help = "filetypes to process (default: gif, jpeg, png). may be specified more than once"
    )]
    pub types: Vec<ImageType>,
    #[arg(long)]
    pub fast: bool,
    #[arg(
        short = 'n',
        long = "num-concurrent",
        value_name = "NUM",
        default_value_t = 0,
        help = "number of parallel jobs, 0 for cpu count"
    )]
    pub num_procs: usize,
    #[arg(
        short = 'x',
        long = "num-concurrent-hash",
        value_name = "NUM",
        help = "number of parallel file-hashing threads, 0 for cpu count \
                (default: --num-concurrent)"
    )]
    pub hash_threads: Option<usize>,
    #[arg(short, long)]
    pub quiet: bool,
    #[arg(
        short,
        long,
        help = "print subprocess stderr (e.g. exiv2 warnings) on error"
    )]
    pub verbose: bool,
    #[arg(
        long,
        help = "disable cache (by default, files already processed are skipped)"
    )]
    pub no_cache: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "path to cache database (default: the nearest img_optim.db above the \
                input paths, else the user's cache directory)"
    )]
    pub cache_path: Option<PathBuf>,
    #[arg(
        long,
        help = "create a new img_optim.db at the input path instead of using the one \
                in the user's cache directory"
    )]
    pub mkdb: bool,
    #[arg(
        long = "hash-db",
        value_name = "DB",
        help = "read-only db consulted by content hash to skip already-optimized files; \
                matches are recorded into the cache. may be repeated"
    )]
    pub hash_dbs: Vec<PathBuf>,
    #[arg(
        short = 'E',
        long = "exclude",
        value_name = "PATTERN",
        help = "exclude paths matching glob pattern (may be repeated)"
    )]
    pub excludes: Vec<String>,
    #[arg(short, long, help = "list files that would be optimized and exit")]
    pub list: bool,
    #[arg(
        short = 'I',
        long,
        help = "do not skip files matched by .gitignore rules"
    )]
    pub no_ignore: bool,
    #[arg(value_name = "PATH", required = true)]
    pub paths: Vec<PathBuf>,
}

/// An output format of `convert`.
#[derive(Clone, Copy, ValueEnum)]
pub enum Format {
    Bmp,
    Jpeg,
    Jxl,
    Png,
    Ppm,
    Tiff,
}

impl Format {
    pub fn mime(self) -> Mime {
        match self {
            Format::Bmp => Mime::Bmp,
            Format::Jpeg => Mime::Jpeg,
            Format::Jxl => Mime::Jxl,
            Format::Png => Mime::Png,
            Format::Ppm => Mime::Ppm,
            Format::Tiff => Mime::Tiff,
        }
    }
}

/// A file type selectable with `optim --type`.
#[derive(Clone, Copy, ValueEnum)]
pub enum ImageType {
    Bmp,
    Gif,
    Heic,
    Jpeg,
    Jxl,
    Png,
    Ppm,
    Svg,
    Tiff,
}

impl ImageType {
    pub fn mime(self) -> Mime {
        match self {
            ImageType::Bmp => Mime::Bmp,
            ImageType::Gif => Mime::Gif,
            ImageType::Heic => Mime::Heif,
            ImageType::Jpeg => Mime::Jpeg,
            ImageType::Jxl => Mime::Jxl,
            ImageType::Png => Mime::Png,
            ImageType::Ppm => Mime::Ppm,
            ImageType::Svg => Mime::Svg,
            ImageType::Tiff => Mime::Tiff,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Kernel {
    Nearest,
    Linear,
    Cubic,
    Mitchell,
    Lanczos2,
    Lanczos3,
}

impl Kernel {
    pub fn vips_name(self) -> &'static str {
        match self {
            Kernel::Nearest => "nearest",
            Kernel::Linear => "linear",
            Kernel::Cubic => "cubic",
            Kernel::Mitchell => "mitchell",
            Kernel::Lanczos2 => "lanczos2",
            Kernel::Lanczos3 => "lanczos3",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ResizeMode {
    Shrink,
    Grow,
    Stretch,
}

impl ResizeMode {
    pub fn vips_size(self) -> &'static str {
        match self {
            ResizeMode::Shrink => "down",
            ResizeMode::Grow => "up",
            ResizeMode::Stretch => "force",
        }
    }
}

fn parse_scale(value: &str) -> Result<f64, String> {
    let number = |text: &str| {
        text.trim()
            .parse::<f64>()
            .map_err(|error| format!("{text:?}: {error}"))
    };
    let scale = if let Some(percent) = value.strip_suffix('%') {
        number(percent)? / 100.0
    } else if let Some((numerator, denominator)) = value.split_once('/') {
        number(numerator)? / number(denominator)?
    } else {
        number(value)?
    };
    if scale > 0.0 && scale.is_finite() {
        Ok(scale)
    } else {
        Err("must be positive".to_owned())
    }
}
