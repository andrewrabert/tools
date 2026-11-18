//! A port of binmerge by Chris Putnam (<https://github.com/putnam/binmerge>).
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{self, Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;

use crate::cue::{self, Command, CueSheet, Filetype, Time};

const BIN_EXTENSION: &str = "bin";
const SCRATCH_PREFIX: &str = ".cue-tool-";

/// What becomes of the sectors before the first INDEX of the first track when splitting.
#[derive(Clone, Copy, ValueEnum)]
pub enum LeadIn {
    /// Refuse to split
    Error,
    /// Keep them at the start of the first track's file
    FirstTrack,
    /// Leave them out
    Discard,
}

#[derive(Clone, Copy)]
pub enum Operation {
    Merge,
    Split(LeadIn),
}

impl Operation {
    fn write(self, source: &Source, outdir: &Path, basename: &str) -> Result<Written> {
        match self {
            Operation::Merge => merge_into(source, outdir, basename),
            Operation::Split(lead_in) => split_into(source, outdir, basename, lead_in),
        }
    }
}

/// A cue sheet and the bin files it lists, which are in its directory.
struct Source {
    cuefile: PathBuf,
    dir: PathBuf,
    sheet: CueSheet,
    bins: Vec<BinFile>,
    sector_size: u64,
}

impl Source {
    fn read(cuefile: &Path) -> Result<Self> {
        let cuefile =
            path::absolute(cuefile).with_context(|| format!("resolving {}", cuefile.display()))?;
        let dir = cuefile
            .parent()
            .with_context(|| format!("{} has no parent", cuefile.display()))?
            .to_owned();
        let data = fs::read(&cuefile).with_context(|| format!("reading {}", cuefile.display()))?;
        let sheet = CueSheet::new(&data).with_context(|| cuefile.display().to_string())?;
        let bins = bin_files(&dir, &sheet)?;
        let sector_size = sector_size(&sheet).with_context(|| cuefile.display().to_string())?;
        Ok(Self {
            cuefile,
            dir,
            sheet,
            bins,
            sector_size,
        })
    }

    /// The file name of the sheet without its extension.
    fn name(&self) -> Result<&str> {
        self.cuefile
            .file_stem()
            .and_then(OsStr::to_str)
            .with_context(|| format!("the name of {} is not UTF-8", self.cuefile.display()))
    }
}

pub struct Written {
    pub bins: Vec<PathBuf>,
    pub cue: PathBuf,
}

/// Where `<basename>.cue` goes in `outdir`, which must exist. Nothing may be there yet.
fn new_cue(outdir: &Path, basename: &str) -> Result<PathBuf> {
    ensure!(
        !basename.is_empty()
            && Path::new(basename)
                .file_name()
                .is_some_and(|name| name == basename),
        "{basename:?} is not a file name"
    );
    let new_cue = outdir.join(format!("{basename}.{}", cue::EXTENSION));
    ensure!(!new_cue.exists(), "{} already exists", new_cue.display());
    Ok(new_cue)
}

/// Merges the bin files of `cuefile` into `<basename>.bin` with a `<basename>.cue` of corrected
/// offsets, in `outdir` or else the sheet's directory. Nothing is overwritten.
pub fn merge(cuefile: &Path, outdir: Option<&Path>, basename: &str) -> Result<()> {
    let source = Source::read(cuefile)?;
    merge_into(&source, outdir.unwrap_or(&source.dir), basename)?;
    Ok(())
}

/// Writes what `operation` makes of `cuefile` to `outdir` or else the sheet's directory, named
/// `name` or else as the sheet is. Nothing is overwritten.
pub fn add(
    cuefile: &Path,
    outdir: Option<&Path>,
    name: Option<&str>,
    operation: Operation,
) -> Result<Written> {
    let source = Source::read(cuefile)?;
    let name = match name {
        Some(name) => name,
        None => source.name()?,
    };
    operation.write(&source, outdir.unwrap_or(&source.dir), name)
}

pub enum Replacement {
    /// The numbers of bin files before and after.
    Replaced { old: usize, new: usize },
    /// The sheet's bin files were already as `operation` leaves them.
    Unneeded,
}

/// Replaces `cuefile` and its bin files with what `operation` makes of them, under the sheet's
/// name. The old files are removed only once the new ones hold as many bytes and are in place.
pub fn replace(cuefile: &Path, operation: Operation) -> Result<Replacement> {
    let source = Source::read(cuefile)?;
    let unneeded = match operation {
        Operation::Merge => source.bins.len() == 1,
        Operation::Split(_) => source.bins.len() == source.sheet.tracks().len(),
    };
    if unneeded {
        return Ok(Replacement::Unneeded);
    }
    let scratch = tempfile::Builder::new()
        .prefix(SCRATCH_PREFIX)
        .tempdir_in(&source.dir)
        .with_context(|| format!("creating a directory in {}", source.dir.display()))?;
    let written = operation.write(&source, scratch.path(), source.name()?)?;

    let old: BTreeSet<&Path> = source.bins.iter().map(|bin| bin.path.as_path()).collect();
    let old_bytes: u64 = source.bins.iter().map(|bin| bin.size).sum();
    let mut new_bytes = 0;
    let mut new = Vec::with_capacity(written.bins.len());
    for bin in &written.bins {
        new_bytes += fs::metadata(bin)
            .with_context(|| format!("reading {}", bin.display()))?
            .len();
        let target = source
            .dir
            .join(bin.file_name().expect("a written file has a name"));
        ensure!(
            old.contains(target.as_path()) || !target.exists(),
            "{} already exists and is not a file of {}",
            target.display(),
            source.cuefile.display()
        );
        new.push(target);
    }
    ensure!(
        new_bytes == old_bytes,
        "the new bin files of {} hold {new_bytes} bytes, not the {old_bytes} of the old",
        source.cuefile.display()
    );

    let rename = |from: &Path, to: &Path| {
        fs::rename(from, to)
            .with_context(|| format!("moving {} to {}", from.display(), to.display()))
    };
    for (bin, target) in written.bins.iter().zip(&new) {
        rename(bin, target)?;
    }
    rename(&written.cue, &source.cuefile)?;
    for path in &old {
        if !new.iter().any(|target| target == path) {
            fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
        }
    }
    Ok(Replacement::Replaced {
        old: old.len(),
        new: new.len(),
    })
}

fn merge_into(source: &Source, outdir: &Path, basename: &str) -> Result<Written> {
    let new_cue = new_cue(outdir, basename)?;
    let new_bin = outdir.join(format!("{basename}.{BIN_EXTENSION}"));
    ensure!(!new_bin.exists(), "{} already exists", new_bin.display());
    let new_sheet = merged_sheet(&source.sheet, &source.bins, source.sector_size, basename)
        .with_context(|| source.cuefile.display().to_string())?;
    let mut writer = BufWriter::new(create(&new_bin)?);
    for bin in &source.bins {
        io::copy(&mut open(&bin.path)?, &mut writer)
            .with_context(|| format!("writing {}", new_bin.display()))?;
    }
    writer
        .flush()
        .with_context(|| format!("writing {}", new_bin.display()))?;
    write_sheet(&new_cue, &new_sheet)?;
    Ok(Written {
        bins: vec![new_bin],
        cue: new_cue,
    })
}

/// One bin file per track, named by the Redump convention from `basename`.
fn split_into(source: &Source, outdir: &Path, basename: &str, lead_in: LeadIn) -> Result<Written> {
    let new_cue = new_cue(outdir, basename)?;
    let [bin] = source.bins.as_slice() else {
        bail!(
            "{} lists {} files; splitting needs exactly one",
            source.cuefile.display(),
            source.bins.len()
        );
    };
    let tracks = split_tracks(&source.sheet, bin, source.sector_size, basename, lead_in)?;
    let outputs: Vec<PathBuf> = tracks
        .iter()
        .map(|track| outdir.join(&track.filename))
        .collect();
    for output in &outputs {
        ensure!(!output.exists(), "{} already exists", output.display());
    }
    let new_sheet = split_sheet(&source.sheet, &tracks)
        .with_context(|| source.cuefile.display().to_string())?;
    let mut input = BufReader::new(open(&bin.path)?);
    let first = tracks.first().expect("a sheet has a track");
    input
        .seek(SeekFrom::Start(first.start * source.sector_size))
        .with_context(|| format!("reading {}", bin.path.display()))?;
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
    write_sheet(&new_cue, &new_sheet)?;
    Ok(Written {
        bins: outputs,
        cue: new_cue,
    })
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
/// track's, or to the end of the file. `lead_in` decides what becomes of sectors before the
/// first INDEX of the first track.
fn split_tracks(
    sheet: &CueSheet,
    bin: &BinFile,
    sector_size: u64,
    basename: &str,
    lead_in: LeadIn,
) -> Result<Vec<SplitTrack>> {
    let total = sectors(bin, sector_size)?;
    let mut starts = sheet
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
    if let Some(first) = starts.first_mut()
        && *first > 0
    {
        match lead_in {
            LeadIn::Error => bail!(
                "the first track starts at sector {first}, not at the start of {}; --lead-in \
                 chooses what becomes of the sectors before it",
                bin.path.display()
            ),
            LeadIn::FirstTrack => *first = 0,
            LeadIn::Discard => {}
        }
    }
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
FILE \"Merged.bin\" BINARY
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
        let tracks = split_tracks(&sheet, &bins(&[600])[0], 2352, "G", LeadIn::Error).unwrap();
        let extents: Vec<(u64, u64)> = tracks.iter().map(|t| (t.start, t.bytes)).collect();
        assert_eq!(
            extents,
            [(0, 100 * 2352), (100, 200 * 2352), (300, 300 * 2352)]
        );
        assert_eq!(split_sheet(&sheet, &tracks).unwrap().to_string(), SPLIT);
    }

    const LEAD_IN: &str = "FILE \"G.bin\" BINARY\n  TRACK 01 AUDIO\n    INDEX 01 00:00:02\n  TRACK 02 AUDIO\n    INDEX 01 00:00:05\n";

    #[test]
    fn split_refuses_sectors_before_the_first_track() {
        let sheet = CueSheet::new(LEAD_IN.as_bytes()).unwrap();
        assert!(split_tracks(&sheet, &bins(&[10])[0], 2352, "G", LeadIn::Error).is_err());
    }

    #[test]
    fn split_leaves_out_sectors_before_the_first_track() {
        let sheet = CueSheet::new(LEAD_IN.as_bytes()).unwrap();
        let tracks = split_tracks(&sheet, &bins(&[10])[0], 2352, "G", LeadIn::Discard).unwrap();
        let extents: Vec<(u64, u64)> = tracks.iter().map(|t| (t.start, t.bytes)).collect();
        assert_eq!(extents, [(2, 3 * 2352), (5, 5 * 2352)]);
        assert_eq!(
            split_sheet(&sheet, &tracks).unwrap().to_string(),
            "FILE \"G (Track 1).bin\" BINARY\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\nFILE \"G (Track 2).bin\" BINARY\n  TRACK 02 AUDIO\n    INDEX 01 00:00:00\n"
        );
    }

    #[test]
    fn split_keeps_sectors_before_the_first_track() {
        let sheet = CueSheet::new(LEAD_IN.as_bytes()).unwrap();
        let tracks = split_tracks(&sheet, &bins(&[10])[0], 2352, "G", LeadIn::FirstTrack).unwrap();
        let extents: Vec<(u64, u64)> = tracks.iter().map(|t| (t.start, t.bytes)).collect();
        assert_eq!(extents, [(0, 5 * 2352), (5, 5 * 2352)]);
        assert_eq!(
            split_sheet(&sheet, &tracks).unwrap().to_string(),
            "FILE \"G (Track 1).bin\" BINARY\n  TRACK 01 AUDIO\n    INDEX 01 00:00:02\nFILE \"G (Track 2).bin\" BINARY\n  TRACK 02 AUDIO\n    INDEX 01 00:00:00\n"
        );
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

    /// A directory holding `G.cue`, a copy of `sheet`, and a bin file of each given name and
    /// sector count, every sector filled with its number in the disc, modulo 256.
    fn disc(sheet: &str, files: &[(&str, u8)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("G.cue"), sheet).unwrap();
        let mut sector = 0_u8;
        for (name, sectors) in files {
            let mut data = Vec::new();
            for _ in 0..*sectors {
                data.extend([sector; 2352]);
                sector = sector.wrapping_add(1);
            }
            fs::write(dir.path().join(name), data).unwrap();
        }
        dir
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    const TRACK_FILES: [(&str, u8); 3] = [
        ("G (Track 1).bin", 100),
        ("G (Track 2).bin", 200),
        ("G (Track 3).bin", 50),
    ];

    #[test]
    fn replace_merge_then_split() {
        let dir = disc(SPLIT, &TRACK_FILES);
        let cuefile = dir.path().join("G.cue");
        let tracks: Vec<Vec<u8>> = TRACK_FILES
            .iter()
            .map(|(name, _)| fs::read(dir.path().join(name)).unwrap())
            .collect();

        let merged = replace(&cuefile, Operation::Merge).unwrap();
        assert!(matches!(merged, Replacement::Replaced { old: 3, new: 1 }));
        assert_eq!(names(dir.path()), ["G.bin", "G.cue"]);
        assert_eq!(fs::read(dir.path().join("G.bin")).unwrap(), tracks.concat());
        assert!(matches!(
            replace(&cuefile, Operation::Merge).unwrap(),
            Replacement::Unneeded
        ));

        let split = replace(&cuefile, Operation::Split(LeadIn::Error)).unwrap();
        assert!(matches!(split, Replacement::Replaced { old: 1, new: 3 }));
        assert_eq!(
            names(dir.path()),
            [
                "G (Track 1).bin",
                "G (Track 2).bin",
                "G (Track 3).bin",
                "G.cue"
            ]
        );
        for ((name, _), track) in TRACK_FILES.iter().zip(&tracks) {
            assert_eq!(&fs::read(dir.path().join(name)).unwrap(), track);
        }
        assert_eq!(fs::read_to_string(&cuefile).unwrap(), SPLIT);
        assert!(matches!(
            replace(&cuefile, Operation::Split(LeadIn::Error)).unwrap(),
            Replacement::Unneeded
        ));
    }

    // The merged bin file takes the name of a file the sheet does not list.
    #[test]
    fn replace_keeps_unlisted_file() {
        let dir = disc(SPLIT, &TRACK_FILES);
        fs::write(dir.path().join("G.bin"), "unlisted").unwrap();
        let before = names(dir.path());
        assert!(replace(&dir.path().join("G.cue"), Operation::Merge).is_err());
        assert_eq!(names(dir.path()), before);
        assert_eq!(fs::read_to_string(dir.path().join("G.cue")).unwrap(), SPLIT);
    }

    // The first track starts past the start of the bin file, so leaving the sectors before it
    // out would drop them.
    #[test]
    fn replace_split_keeps_every_byte() {
        let dir = disc(LEAD_IN, &[("G.bin", 10)]);
        let cuefile = dir.path().join("G.cue");
        let before = names(dir.path());
        assert!(replace(&cuefile, Operation::Split(LeadIn::Discard)).is_err());
        assert!(replace(&cuefile, Operation::Split(LeadIn::Error)).is_err());
        assert_eq!(names(dir.path()), before);

        replace(&cuefile, Operation::Split(LeadIn::FirstTrack)).unwrap();
        let track = fs::read(dir.path().join("G (Track 1).bin")).unwrap();
        assert_eq!(track.len(), 5 * 2352);
        assert_eq!((track[0], track[5 * 2352 - 1]), (0, 4));
    }
}
