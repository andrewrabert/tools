//! clap argument definitions.
use std::path::PathBuf;

use clap::{ArgGroup, Args as ClapArgs, Subcommand};

#[derive(ClapArgs)]
#[command(
    about = "Check, format, print, and list cue sheets, merge or split their files, and extract their tracks"
)]
pub struct CueTool {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "check cue sheets and that each file a sheet lists exists")]
    Check(Check),
    #[command(about = "print cue sheets in a uniform layout")]
    Fmt(Fmt),
    #[command(about = "print the tokens of cue sheets as JSON, pretty-printed on a terminal")]
    Json(Json),
    #[command(about = "print the absolute path of each file cue sheets list")]
    Files(Files),
    #[command(
        about = "merge the files of each cue sheet into one",
        long_about = "Merge the bin files of each cue sheet into one bin file with a cue sheet \
                      of corrected offsets, as Redump discs with a file per track need for \
                      software that reads a single bin file. Without --in-place, the input \
                      files are left untouched and nothing is overwritten."
    )]
    FileMerge(BinFiles),
    #[command(
        about = "split the single file of each cue sheet into one per track",
        long_about = "Split the single bin file of each cue sheet into one bin file per track, \
                      named by the Redump convention, with a cue sheet of corrected offsets. \
                      Without --in-place, the input files are left untouched and nothing is \
                      overwritten."
    )]
    FileSplit(BinFiles),
    #[command(
        about = "extract each track of a cue sheet's single file as an ISO or audio file",
        long_about = "Extract each track of the single bin file of a cue sheet: data tracks as \
                      ISO files of the data in each sector, audio tracks as headerless CDR or \
                      WAV files, and tracks of any other mode whole as ugh files. The files are \
                      named by track number, as in NAME01.iso. A track runs from its last INDEX \
                      to before the first INDEX of the next track, which leaves pregaps out. \
                      The FILE lines of the cue sheet are not read. Without --force, nothing \
                      is overwritten."
    )]
    TrackExtract(TrackExtract),
}

#[derive(ClapArgs)]
pub struct Input {
    #[arg(
        value_name = "PATH",
        help = "a cue sheet, or a directory with --recursive; stdin when absent"
    )]
    pub path: Option<PathBuf>,
    #[arg(
        short,
        long,
        requires = "path",
        help = "search the directory PATH for .cue files"
    )]
    pub recursive: bool,
}

#[derive(ClapArgs)]
pub struct Check {
    #[command(flatten)]
    pub input: Input,
    #[arg(
        short,
        long,
        help = "check that each cue sheet parses; listed files are not checked"
    )]
    pub parse_only: bool,
    #[arg(
        long,
        requires = "path",
        help = "rewrite each cue sheet that is not UTF-8 without a byte-order mark as UTF-8"
    )]
    pub fix_encoding: bool,
    #[arg(long, help = "exit zero even when a cue sheet has errors")]
    pub exit_zero: bool,
}

#[derive(ClapArgs)]
pub struct Fmt {
    #[command(flatten)]
    pub input: Input,
    #[arg(
        short,
        long,
        requires = "path",
        required_if_eq("recursive", "true"),
        help = "rewrite each cue sheet instead of printing it; required by --recursive"
    )]
    pub in_place: bool,
}

#[derive(ClapArgs)]
pub struct Json {
    #[command(flatten)]
    pub input: Input,
}

#[derive(ClapArgs)]
pub struct Files {
    #[command(flatten)]
    pub input: Input,
    #[arg(short = '0', help = "separate paths with NUL instead of newline")]
    pub null: bool,
}

#[derive(ClapArgs)]
#[command(group(
    ArgGroup::new("destination")
        .required(true)
        .multiple(true)
        .args(["in_place", "outdir", "name"])
))]
pub struct BinFiles {
    #[arg(
        value_name = "CUEFILE",
        required = true,
        help = "a cue sheet; its files are expected in its directory"
    )]
    pub cuefiles: Vec<PathBuf>,
    #[arg(
        short,
        long,
        conflicts_with_all = ["outdir", "name"],
        help = "replace each cue sheet and its files with the result, skipping a cue sheet that \
                needs no change"
    )]
    pub in_place: bool,
    #[arg(
        short,
        long,
        value_name = "DIR",
        help = "write to DIR, created if missing, instead of the directory of CUEFILE"
    )]
    pub outdir: Option<PathBuf>,
    #[arg(
        short,
        long,
        value_name = "NAME",
        help = "name the new files NAME, without extension, instead of as CUEFILE is named; for \
                a single CUEFILE"
    )]
    pub name: Option<String>,
}

#[derive(ClapArgs)]
pub struct TrackExtract {
    #[arg(value_name = "CUEFILE", help = "a cue sheet")]
    pub cuefile: PathBuf,
    #[arg(
        short,
        long,
        value_name = "BIN",
        help = "the bin file CUEFILE describes, instead of CUEFILE with the extension bin"
    )]
    pub bin: Option<PathBuf>,
    #[arg(
        short,
        long,
        value_name = "DIR",
        help = "write to DIR, created if missing, instead of the directory of CUEFILE"
    )]
    pub outdir: Option<PathBuf>,
    #[arg(
        short,
        long,
        value_name = "NAME",
        help = "start the name of each track file with NAME, instead of as CUEFILE is named"
    )]
    pub name: Option<String>,
    #[arg(
        long,
        conflicts_with = "psx",
        help = "write all 2352 bytes of each MODE2/2352 sector (VCD/MPEG), instead of the 2048 \
                bytes at offset 24"
    )]
    pub raw: bool,
    #[arg(
        long,
        help = "write the first 2336 bytes of each MODE2/2352 sector (PSX), instead of the 2048 \
                bytes at offset 24"
    )]
    pub psx: bool,
    #[arg(long, help = "write audio tracks as WAV instead of headerless CDR")]
    pub wav: bool,
    #[arg(long, help = "swap the byte order of audio samples")]
    pub swap_audio: bool,
    #[arg(short, long, help = "overwrite track files that exist")]
    pub force: bool,
}
