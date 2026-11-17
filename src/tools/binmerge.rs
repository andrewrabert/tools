//! A port of binmerge by Chris Putnam (<https://github.com/putnam/binmerge>).
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{self, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail, ensure};
use clap::Args as ClapArgs;

use crate::cue::{self, Command, CueSheet, Filetype, Time};
use crate::tools::Tool;

const BIN_EXTENSION: &str = "bin";

#[derive(ClapArgs)]
#[command(
    about = "Merge the bin files of a cue sheet into one, or split one back into tracks",
    long_about = "Merge the bin files of a cue sheet into one bin file with a cue sheet of \
                  corrected offsets, as Redump discs with a file per track need for software \
                  that reads a single bin file. With --split, the reverse: one bin file per \
                  track, named by the Redump convention. The input files are left untouched."
)]
pub struct Binmerge {
    /// Split the single bin file of the cue sheet into one bin file per track
    #[arg(short, long)]
    split: bool,

    /// Directory to write to, created if missing [default: the directory of CUEFILE]
    #[arg(short, long, value_name = "DIR")]
    outdir: Option<PathBuf>,

    /// The cue sheet; its bin files are expected in the same directory
    #[arg(value_name = "CUEFILE")]
    cuefile: PathBuf,

    /// Name of the new bin and cue files, without extension
    #[arg(value_name = "BASENAME")]
    basename: String,
}

impl Tool for Binmerge {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:#}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: Binmerge) -> Result<()> {
    let outdir = match args.outdir {
        Some(outdir) => {
            fs::create_dir_all(&outdir)
                .with_context(|| format!("creating {}", outdir.display()))?;
            Some(outdir)
        }
        None => None,
    };
    let written = if args.split {
        split(&args.cuefile, outdir.as_deref(), &args.basename)?
    } else {
        merge(&args.cuefile, outdir.as_deref(), &args.basename)?
    };
    for path in written {
        println!("Wrote {}", path.display());
    }
    Ok(())
}

struct Job {
    cuefile: PathBuf,
    outdir: PathBuf,
    sheet: CueSheet,
    bins: Vec<BinFile>,
    sector_size: u64,
    new_cue: PathBuf,
}

/// Reads `cuefile` and its bin files, and checks `basename` is free in `outdir`, which defaults
/// to the sheet's directory and must exist.
fn job(cuefile: &Path, outdir: Option<&Path>, basename: &str) -> Result<Job> {
    ensure!(
        !basename.is_empty()
            && Path::new(basename)
                .file_name()
                .is_some_and(|name| name == basename),
        "{basename:?} is not a file name"
    );
    let cuefile =
        path::absolute(cuefile).with_context(|| format!("resolving {}", cuefile.display()))?;
    let cue_dir = cuefile
        .parent()
        .with_context(|| format!("{} has no parent", cuefile.display()))?;
    let outdir = outdir.unwrap_or(cue_dir).to_owned();
    let data = fs::read(&cuefile).with_context(|| format!("reading {}", cuefile.display()))?;
    let sheet = CueSheet::new(&data).with_context(|| cuefile.display().to_string())?;
    let bins = bin_files(cue_dir, &sheet)?;
    let sector_size = sector_size(&sheet)?;
    let new_cue = outdir.join(format!("{basename}.{}", cue::EXTENSION));
    ensure!(!new_cue.exists(), "{} already exists", new_cue.display());
    Ok(Job {
        cuefile,
        outdir,
        sheet,
        bins,
        sector_size,
        new_cue,
    })
}

/// Merges the bin files of `cuefile` into `<basename>.bin` with a `<basename>.cue` of corrected
/// offsets, in `outdir` or else the sheet's directory. Returns the paths written. Nothing is
/// overwritten.
pub fn merge(cuefile: &Path, outdir: Option<&Path>, basename: &str) -> Result<Vec<PathBuf>> {
    let job = job(cuefile, outdir, basename)?;
    let new_bin = job.outdir.join(format!("{basename}.{BIN_EXTENSION}"));
    ensure!(!new_bin.exists(), "{} already exists", new_bin.display());
    let new_sheet = merged_sheet(&job.sheet, &job.bins, job.sector_size, basename)?;
    let mut writer = BufWriter::new(create(&new_bin)?);
    for bin in &job.bins {
        io::copy(&mut open(&bin.path)?, &mut writer)
            .with_context(|| format!("writing {}", new_bin.display()))?;
    }
    writer
        .flush()
        .with_context(|| format!("writing {}", new_bin.display()))?;
    write_sheet(&job.new_cue, &new_sheet)?;
    Ok(vec![new_bin, job.new_cue])
}

/// Splits the single bin file of `cuefile` into one per track, named by the Redump convention
/// from `basename`, with a `<basename>.cue`, in `outdir` or else the sheet's directory. Returns
/// the paths written. Nothing is overwritten.
pub fn split(cuefile: &Path, outdir: Option<&Path>, basename: &str) -> Result<Vec<PathBuf>> {
    let job = job(cuefile, outdir, basename)?;
    let [bin] = job.bins.as_slice() else {
        bail!(
            "{} lists {} files; splitting needs exactly one",
            job.cuefile.display(),
            job.bins.len()
        );
    };
    let tracks = split_tracks(&job.sheet, bin, job.sector_size, basename)?;
    let outputs: Vec<PathBuf> = tracks
        .iter()
        .map(|track| job.outdir.join(&track.filename))
        .collect();
    for output in &outputs {
        ensure!(!output.exists(), "{} already exists", output.display());
    }
    let new_sheet = split_sheet(&job.sheet, &tracks)?;
    let mut input = BufReader::new(open(&bin.path)?);
    for (track, output) in tracks.iter().zip(&outputs) {
        let mut writer = BufWriter::new(create(output)?);
        let copied = io::copy(&mut (&mut input).take(track.bytes), &mut writer)
            .with_context(|| format!("writing {}", output.display()))?;
        ensure!(
            copied == track.bytes,
            "{} ended {} bytes early",
            bin.path.display(),
            track.bytes - copied
        );
        writer
            .flush()
            .with_context(|| format!("writing {}", output.display()))?;
    }
    write_sheet(&job.new_cue, &new_sheet)?;
    let mut written = outputs;
    written.push(job.new_cue);
    Ok(written)
}

fn open(path: &Path) -> Result<File> {
    File::open(path).with_context(|| format!("opening {}", path.display()))
}

fn create(path: &Path) -> Result<File> {
    File::create_new(path).with_context(|| format!("creating {}", path.display()))
}

fn write_sheet(path: &Path, sheet: &CueSheet) -> Result<()> {
    let mut file = create(path)?;
    file.write_all(sheet.to_string().as_bytes())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

struct BinFile {
    path: PathBuf,
    size: u64,
}

/// The files the sheet lists, in `dir`. Every file must be BINARY and exist.
fn bin_files(dir: &Path, sheet: &CueSheet) -> Result<Vec<BinFile>> {
    sheet
        .files()
        .iter()
        .map(|file| {
            ensure!(
                matches!(file.filetype(), Filetype::Binary),
                "{:?} is {}, not BINARY",
                file.filename(),
                file.filetype()
            );
            let path = dir.join(file.filename());
            let size = fs::metadata(&path)
                .with_context(|| format!("reading {}", path.display()))?
                .len();
            Ok(BinFile { path, size })
        })
        .collect()
}

/// The sector size the sheet's tracks share.
fn sector_size(sheet: &CueSheet) -> Result<u64> {
    let mut sizes = sheet
        .tracks()
        .iter()
        .map(|track| track.datatype().sector_size());
    let size = sizes.next().expect("a sheet has a track");
    ensure!(
        sizes.all(|other| other == size),
        "the tracks have differing sector sizes, so one bin file cannot hold them all"
    );
    Ok(size)
}

fn sectors(bin: &BinFile, sector_size: u64) -> Result<u64> {
    ensure!(
        bin.size.is_multiple_of(sector_size),
        "{} is {} bytes, not a multiple of the {sector_size}-byte sector",
        bin.path.display(),
        bin.size
    );
    Ok(bin.size / sector_size)
}

fn time(frames: u64) -> Result<Time> {
    Time::from_frames(frames).with_context(|| format!("{frames} frames is past 99:59:74"))
}

/// The sheet with every file replaced by `<basename>.bin`, each INDEX moved by the sectors of
/// the files before its own.
fn merged_sheet(
    sheet: &CueSheet,
    bins: &[BinFile],
    sector_size: u64,
    basename: &str,
) -> Result<CueSheet> {
    let mut offsets = Vec::with_capacity(bins.len());
    let mut offset = 0;
    for bin in bins {
        offsets.push(offset);
        offset += sectors(bin, sector_size)?;
    }
    let mut commands = Vec::with_capacity(sheet.commands().len());
    let mut tracks = sheet.tracks().iter();
    let mut track = None;
    let mut file_written = false;
    for (command, parameters) in sheet.commands() {
        let parameters: Vec<String> = match command {
            Command::File if file_written => continue,
            Command::File => {
                file_written = true;
                vec![
                    format!("{basename}.{BIN_EXTENSION}"),
                    Filetype::Binary.to_string(),
                ]
            }
            Command::Track => {
                track = tracks.next();
                parameters.iter().map(|p| p.as_str().to_owned()).collect()
            }
            Command::Index => {
                let track = track.expect("an INDEX follows a TRACK");
                let [number, old] = parameters.as_slice() else {
                    unreachable!("an INDEX has two parameters");
                };
                let frames = Time::try_from(old.as_str())
                    .expect("a parsed INDEX time")
                    .frames();
                let moved = time(offsets[track.file()] + frames)?;
                vec![number.as_str().to_owned(), moved.to_string()]
            }
            _ => parameters.iter().map(|p| p.as_str().to_owned()).collect(),
        };
        commands.push((*command, parameters));
    }
    Ok(CueSheet::from_commands(&commands)?)
}

struct SplitTrack {
    filename: String,
    /// The sectors from the start of the bin file.
    start: u64,
    bytes: u64,
}

/// Each track's file name and extent in `bin`. A track runs from its first INDEX to the next
/// track's, or to the end of the file.
fn split_tracks(
    sheet: &CueSheet,
    bin: &BinFile,
    sector_size: u64,
    basename: &str,
) -> Result<Vec<SplitTrack>> {
    let total = sectors(bin, sector_size)?;
    let starts = sheet
        .tracks()
        .iter()
        .map(|track| {
            track
                .indexes()
                .first()
                .map(|index| index.time().frames())
                .with_context(|| format!("track {} has no INDEX", track.number()))
        })
        .collect::<Result<Vec<_>>>()?;
    let count = sheet.tracks().len();
    sheet
        .tracks()
        .iter()
        .enumerate()
        .map(|(position, track)| {
            let start = starts[position];
            let end = starts.get(position + 1).copied().unwrap_or(total);
            ensure!(
                start <= end && end <= total,
                "track {} starts at sector {start}, past the end of {}",
                track.number(),
                bin.path.display()
            );
            Ok(SplitTrack {
                filename: track_filename(basename, track.number(), count),
                start,
                bytes: (end - start) * sector_size,
            })
        })
        .collect()
}

/// The Redump file name of a track: a lone track is `<basename>.bin`, otherwise
/// `<basename> (Track N).bin`, padding N to two digits past nine tracks.
fn track_filename(basename: &str, number: u8, count: usize) -> String {
    match count {
        1 => format!("{basename}.{BIN_EXTENSION}"),
        2..=9 => format!("{basename} (Track {number}).{BIN_EXTENSION}"),
        _ => format!("{basename} (Track {number:02}).{BIN_EXTENSION}"),
    }
}

/// The sheet with a FILE before every TRACK, each INDEX moved back to the start of its track.
fn split_sheet(sheet: &CueSheet, tracks: &[SplitTrack]) -> Result<CueSheet> {
    let mut commands = Vec::with_capacity(sheet.commands().len() + tracks.len());
    let mut remaining = tracks.iter();
    let mut track = None;
    for (command, parameters) in sheet.commands() {
        let parameters: Vec<String> = match command {
            Command::File => continue,
            Command::Track => {
                let next = remaining.next().expect("a split track per TRACK");
                track = Some(next);
                commands.push((
                    Command::File,
                    vec![next.filename.clone(), Filetype::Binary.to_string()],
                ));
                parameters.iter().map(|p| p.as_str().to_owned()).collect()
            }
            Command::Index => {
                let track = track.expect("an INDEX follows a TRACK");
                let [number, old] = parameters.as_slice() else {
                    unreachable!("an INDEX has two parameters");
                };
                let frames = Time::try_from(old.as_str())
                    .expect("a parsed INDEX time")
                    .frames();
                let moved = time(frames - track.start)?;
                vec![number.as_str().to_owned(), moved.to_string()]
            }
            _ => parameters.iter().map(|p| p.as_str().to_owned()).collect(),
        };
        commands.push((*command, parameters));
    }
    Ok(CueSheet::from_commands(&commands)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPLIT: &str = "\
REM COMMENT hi
FILE \"G (Track 1).bin\" BINARY
  TRACK 01 MODE2/2352
    INDEX 01 00:00:00
FILE \"G (Track 2).bin\" BINARY
  TRACK 02 AUDIO
    INDEX 00 00:00:00
    INDEX 01 00:02:00
FILE \"G (Track 3).bin\" BINARY
  TRACK 03 AUDIO
    INDEX 00 00:00:00
    INDEX 01 00:02:00
";

    const MERGED: &str = "\
REM COMMENT hi
FILE Merged.bin BINARY
  TRACK 01 MODE2/2352
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    INDEX 00 00:01:25
    INDEX 01 00:03:25
  TRACK 03 AUDIO
    INDEX 00 00:04:00
    INDEX 01 00:06:00
";

    fn bins(sectors: &[u64]) -> Vec<BinFile> {
        sectors
            .iter()
            .enumerate()
            .map(|(i, sectors)| BinFile {
                path: PathBuf::from(format!("{i}.bin")),
                size: sectors * 2352,
            })
            .collect()
    }

    #[test]
    fn merge() {
        let sheet = CueSheet::new(SPLIT.as_bytes()).unwrap();
        let merged = merged_sheet(&sheet, &bins(&[100, 200, 300]), 2352, "Merged").unwrap();
        assert_eq!(merged.to_string(), MERGED);
    }

    #[test]
    fn split() {
        let sheet = CueSheet::new(MERGED.as_bytes()).unwrap();
        let tracks = split_tracks(&sheet, &bins(&[600])[0], 2352, "G").unwrap();
        let extents: Vec<(u64, u64)> = tracks.iter().map(|t| (t.start, t.bytes)).collect();
        assert_eq!(
            extents,
            [(0, 100 * 2352), (100, 200 * 2352), (300, 300 * 2352)]
        );
        assert_eq!(split_sheet(&sheet, &tracks).unwrap().to_string(), SPLIT);
    }

    #[test]
    fn partial_sector() {
        let sheet = CueSheet::new(SPLIT.as_bytes()).unwrap();
        let mut files = bins(&[1, 1, 1]);
        files[1].size += 1;
        assert!(merged_sheet(&sheet, &files, 2352, "Merged").is_err());
    }

    #[test]
    fn track_filenames() {
        assert_eq!(track_filename("G", 1, 1), "G.bin");
        assert_eq!(track_filename("G", 2, 9), "G (Track 2).bin");
        assert_eq!(track_filename("G", 2, 10), "G (Track 02).bin");
    }
}
