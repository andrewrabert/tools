//! Splits a FLAC file into one FLAC file per track of its cue sheet.
//!
//! With [`LeadIn::Separate`] and [`Gaps::Append`] the pieces are those of
//! `shntool split -f CUE -t %n` (shntool 3.0.10 by Jason Jordan): the file is cut at the
//! `INDEX 01` of each track, audio before the first track is a piece numbered 00, and the pieces
//! carry no tags. Unlike the original, a frame is 1/75 of a second at any sample rate, a track
//! without an `INDEX 01` is an error, and so is a sheet of several files.
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;
use tempfile::Builder;

use crate::cue::{CueSheet, Time};
use crate::pool;

const EXTENSION: &str = "flac";
const NAME_PREFIX: &str = "split";
const SCRATCH_PREFIX: &str = ".flac-split";
/// The number of the `INDEX` a track's audio starts at.
const START_INDEX: u8 = 1;
/// The number of the `INDEX` the gap before a track's audio starts at.
const GAP_INDEX: u8 = 0;

/// What becomes of the audio before the first track.
#[derive(Clone, Copy, ValueEnum)]
pub enum LeadIn {
    /// Refuse to split
    Error,
    /// Put it at the start of the first track
    FirstTrack,
    /// Write it as split00.flac
    Separate,
    /// Leave it out
    Discard,
}

/// What becomes of the gap from the INDEX 00 of a track to its INDEX 01. The gap of the first
/// track has no track before it, so unless it is prepended it is audio before the first track.
#[derive(Clone, Copy, ValueEnum)]
pub enum Gaps {
    /// Put it at the end of the track before
    Append,
    /// Put it at the start of its own track
    Prepend,
    /// Leave it out
    Discard,
}

#[derive(Clone, Copy)]
pub struct Options {
    pub lead_in: LeadIn,
    pub gaps: Gaps,
}

/// The file has audio before the first track and [`LeadIn::Error`] was chosen.
#[derive(Debug)]
pub struct LeadInError {
    /// The sample the first track starts at.
    sample: u64,
}

impl fmt::Display for LeadInError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the first track starts at sample {}, not at the start of the file; --lead-in chooses \
             what becomes of the audio before it",
            self.sample
        )
    }
}

impl std::error::Error for LeadInError {}

/// A run of samples that becomes one file.
#[derive(Debug, PartialEq)]
struct Piece {
    number: usize,
    /// The first sample.
    start: u64,
    /// The sample after the last.
    end: u64,
}

impl Piece {
    fn filename(&self) -> String {
        format!("{NAME_PREFIX}{:02}.{EXTENSION}", self.number)
    }
}

/// Splits `flac` at the tracks of `sheet` into `split01.flac`, `split02.flac`, and so on in
/// `outdir`, encoded in parallel. `flac` is left as it is and nothing is overwritten.
///
/// # Errors
///
/// A [`LeadInError`] when the file has audio before the first track and `options` refuses it.
pub fn tracks(
    flac: &Path,
    sheet: &CueSheet,
    outdir: &Path,
    options: Options,
) -> Result<Vec<PathBuf>> {
    let stream = Stream::read(flac)?;
    let pieces = pieces(sheet, stream.sample_rate, stream.samples, options)
        .with_context(|| format!("splitting {}", flac.display()))?;
    let outputs: Vec<PathBuf> = pieces
        .iter()
        .map(|piece| outdir.join(piece.filename()))
        .collect();
    for output in &outputs {
        ensure!(!output.exists(), "{} already exists", output.display());
    }
    // The pieces are moved into place only once every one of them is whole.
    let scratch = Builder::new()
        .prefix(SCRATCH_PREFIX)
        .tempdir_in(outdir)
        .with_context(|| format!("creating a temporary directory in {}", outdir.display()))?;
    pool::run(
        pieces,
        pool::jobs(0)?,
        |piece| encode(flac, piece, &scratch.path().join(piece.filename())),
        |piece, result| result.with_context(|| format!("writing {}", piece.filename())),
    )?;
    for output in &outputs {
        let name = output.file_name().expect("an output has a file name");
        fs::rename(scratch.path().join(name), output)
            .with_context(|| format!("moving into place {}", output.display()))?;
    }
    Ok(outputs)
}

/// What a FLAC file's STREAMINFO block says of its audio.
struct Stream {
    sample_rate: u64,
    samples: u64,
}

impl Stream {
    fn read(flac: &Path) -> Result<Self> {
        let output = Command::new("metaflac")
            .args(["--show-sample-rate", "--show-total-samples", "--"])
            .arg(flac)
            .stderr(Stdio::inherit())
            .output()
            .context("running metaflac")?;
        if !output.status.success() {
            bail!("metaflac failed: {}", output.status);
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut numbers = text.lines().map(|line| line.trim().parse::<u64>());
        let (Some(Ok(sample_rate)), Some(Ok(samples)), None) =
            (numbers.next(), numbers.next(), numbers.next())
        else {
            bail!("metaflac printed {text:?}, not a sample rate and a sample count");
        };
        ensure!(
            sample_rate > 0 && samples > 0,
            "{} does not record its sample rate and length",
            flac.display()
        );
        Ok(Stream {
            sample_rate,
            samples,
        })
    }
}

/// Where a track's piece begins and where the piece of the track before ends, in samples.
struct Cut {
    track: u8,
    previous_end: u64,
    begin: u64,
}

/// The pieces of a file of `samples` samples: one per track, numbered from 1, the last to the
/// end of the file. `options` decides which piece the gap before a track goes to, and whether
/// audio before the first track is a piece numbered 0. A last track that starts at the end of
/// the file has no piece. A time between two samples is the sample before it.
fn pieces(
    sheet: &CueSheet,
    sample_rate: u64,
    samples: u64,
    options: Options,
) -> Result<Vec<Piece>> {
    ensure!(
        sheet.files().len() == 1,
        "the cue sheet lists {} files; splitting needs exactly one",
        sheet.files().len()
    );
    let mut cuts = Vec::with_capacity(sheet.tracks().len());
    for track in sheet.tracks() {
        let sample = |number: u8| {
            track
                .indexes()
                .iter()
                .find(|index| index.number() == number)
                .map(|index| index.time().frames() * sample_rate / Time::FRAMES_PER_SECOND)
        };
        let start = sample(START_INDEX)
            .with_context(|| format!("track {} has no INDEX {START_INDEX:02}", track.number()))?;
        let gap = sample(GAP_INDEX).unwrap_or(start);
        ensure!(
            gap <= start,
            "track {} has its INDEX {GAP_INDEX:02} after its INDEX {START_INDEX:02}",
            track.number()
        );
        ensure!(
            start <= samples,
            "track {} starts at sample {start}, past the {samples} samples of the file",
            track.number()
        );
        let (previous_end, begin) = match options.gaps {
            Gaps::Append => (start, start),
            Gaps::Prepend => (gap, gap),
            Gaps::Discard => (gap, start),
        };
        cuts.push(Cut {
            track: track.number(),
            previous_end,
            begin,
        });
    }

    let mut pieces = Vec::with_capacity(cuts.len() + 1);
    let first = cuts.first_mut().expect("a sheet has a track");
    if first.begin > 0 {
        match options.lead_in {
            LeadIn::Error => {
                return Err(LeadInError {
                    sample: first.begin,
                }
                .into());
            }
            LeadIn::FirstTrack => first.begin = 0,
            LeadIn::Separate => pieces.push(Piece {
                number: 0,
                start: 0,
                end: first.begin,
            }),
            LeadIn::Discard => {}
        }
    }
    for (position, cut) in cuts.iter().enumerate() {
        let end = match cuts.get(position + 1) {
            Some(next) => next.previous_end,
            None if cut.begin == samples => break,
            None => samples,
        };
        ensure!(
            end > cut.begin,
            "track {} does not end after it starts",
            cut.track
        );
        pieces.push(Piece {
            number: position + 1,
            start: cut.begin,
            end,
        });
    }
    Ok(pieces)
}

/// Writes the samples of `piece` in `flac` to `dest`.
fn encode(flac: &Path, piece: &Piece, dest: &Path) -> Result<()> {
    let mut decoder = Command::new("flac")
        .args(["--decode", "--stdout", "--silent"])
        .arg(format!("--skip={}", piece.start))
        .arg(format!("--until={}", piece.end))
        .arg("--")
        .arg(flac)
        .stdout(Stdio::piped())
        .spawn()
        .context("running the flac decoder")?;
    let wav = decoder.stdout.take().context("the decoder has no stdout")?;
    let encoded = Command::new("flac")
        .args(["--silent", "--best", "--verify", "--output-name"])
        .arg(dest)
        .arg("-")
        .stdin(Stdio::from(wav))
        .status();
    let decoded = decoder.wait().context("waiting for the flac decoder")?;
    let encoded = encoded.context("running the flac encoder")?;
    if !decoded.success() {
        bail!("decoding failed: {decoded}");
    }
    if !encoded.success() {
        bail!("encoding failed: {encoded}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHNTOOL: Options = Options {
        lead_in: LeadIn::Separate,
        gaps: Gaps::Append,
    };

    /// A sheet of one file with a track at each `INDEX 01` time.
    fn sheet(starts: &[&str]) -> CueSheet {
        let mut text = String::from("FILE \"album.flac\" WAVE\n");
        for (position, start) in starts.iter().enumerate() {
            text.push_str(&format!(
                "  TRACK {:02} AUDIO\n    INDEX 01 {start}\n",
                position + 1
            ));
        }
        CueSheet::new(text.as_bytes()).unwrap_or_else(|error| panic!("{error}"))
    }

    /// Tracks at 0:00, 0:07.13 and 0:15.02, the first with a gap of 32 frames before it and the
    /// second with a gap from 0:05.
    fn gapped() -> CueSheet {
        CueSheet::new(
            b"FILE \"album.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 00 00:00:00\n    INDEX 01 00:00:32\n  TRACK 02 AUDIO\n    INDEX 00 00:05:00\n    INDEX 01 00:07:13\n  TRACK 03 AUDIO\n    INDEX 01 00:15:02\n",
        )
        .unwrap_or_else(|error| panic!("{error}"))
    }

    fn piece(number: usize, start: u64, end: u64) -> Piece {
        Piece { number, start, end }
    }

    #[test]
    fn a_piece_per_track_from_its_index_to_the_next() -> Result<()> {
        let sheet = sheet(&["00:00:00", "00:07:13", "00:15:02"]);

        let pieces = pieces(&sheet, 44100, 882_000, SHNTOOL)?;

        assert_eq!(
            pieces,
            [
                piece(1, 0, 316_344),
                piece(2, 316_344, 662_676),
                piece(3, 662_676, 882_000)
            ]
        );
        Ok(())
    }

    #[test]
    fn a_frame_is_a_75th_of_a_second_at_any_sample_rate() -> Result<()> {
        let sheet = sheet(&["00:00:00", "00:07:13"]);

        let pieces = pieces(&sheet, 48000, 960_000, SHNTOOL)?;

        assert_eq!(pieces, [piece(1, 0, 344_320), piece(2, 344_320, 960_000)]);
        Ok(())
    }

    #[test]
    fn a_time_between_samples_is_the_sample_before() -> Result<()> {
        let sheet = sheet(&["00:00:00", "00:00:13"]);

        let pieces = pieces(&sheet, 8000, 160_000, SHNTOOL)?;

        assert_eq!(pieces, [piece(1, 0, 1386), piece(2, 1386, 160_000)]);
        Ok(())
    }

    #[test]
    fn audio_before_the_first_track_is_refused() {
        let sheet = sheet(&["00:00:32", "00:07:13"]);
        let options = Options {
            lead_in: LeadIn::Error,
            ..SHNTOOL
        };

        let error = pieces(&sheet, 44100, 882_000, options)
            .context("splitting")
            .expect_err("audio before the first track");

        assert!(error.downcast_ref::<LeadInError>().is_some());
    }

    #[test]
    fn audio_before_the_first_track_goes_where_lead_in_says() -> Result<()> {
        let sheet = sheet(&["00:00:32", "00:07:13"]);
        let split = |lead_in| pieces(&sheet, 44100, 882_000, Options { lead_in, ..SHNTOOL });
        let second = || piece(2, 316_344, 882_000);

        assert_eq!(
            split(LeadIn::Separate)?,
            [piece(0, 0, 18_816), piece(1, 18_816, 316_344), second()]
        );
        assert_eq!(split(LeadIn::FirstTrack)?, [piece(1, 0, 316_344), second()]);
        assert_eq!(
            split(LeadIn::Discard)?,
            [piece(1, 18_816, 316_344), second()]
        );
        Ok(())
    }

    #[test]
    fn a_gap_goes_where_gaps_says() -> Result<()> {
        let sheet = gapped();
        let split = |gaps| pieces(&sheet, 44100, 882_000, Options { gaps, ..SHNTOOL });
        let third = || piece(3, 662_676, 882_000);

        assert_eq!(
            split(Gaps::Append)?,
            [
                piece(0, 0, 18_816),
                piece(1, 18_816, 316_344),
                piece(2, 316_344, 662_676),
                third()
            ]
        );
        assert_eq!(
            split(Gaps::Prepend)?,
            [piece(1, 0, 220_500), piece(2, 220_500, 662_676), third()]
        );
        assert_eq!(
            split(Gaps::Discard)?,
            [
                piece(0, 0, 18_816),
                piece(1, 18_816, 220_500),
                piece(2, 316_344, 662_676),
                third()
            ]
        );
        Ok(())
    }

    #[test]
    fn a_last_track_at_the_end_of_the_file_has_no_piece() -> Result<()> {
        let sheet = sheet(&["00:00:00", "00:20:00"]);

        let pieces = pieces(&sheet, 44100, 882_000, SHNTOOL)?;

        assert_eq!(pieces, [piece(1, 0, 882_000)]);
        Ok(())
    }

    #[test]
    fn a_track_past_the_end_of_the_file_is_an_error() {
        let sheet = sheet(&["00:00:00", "00:20:01"]);

        assert!(pieces(&sheet, 44100, 882_000, SHNTOOL).is_err());
    }

    #[test]
    fn tracks_must_start_in_order() {
        let split = |starts: &[&str]| pieces(&sheet(starts), 44100, 882_000, SHNTOOL);

        assert!(split(&["00:00:00", "00:07:13", "00:07:13"]).is_err());
        assert!(split(&["00:00:00", "00:00:00"]).is_err());
    }

    #[test]
    fn a_track_without_index_01_is_an_error() -> Result<()> {
        let sheet =
            CueSheet::new(b"FILE \"album.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 00 00:00:00\n")?;

        assert!(pieces(&sheet, 44100, 882_000, SHNTOOL).is_err());
        Ok(())
    }

    #[test]
    fn a_sheet_of_several_files_is_an_error() -> Result<()> {
        let sheet = CueSheet::new(
            b"FILE \"a.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\nFILE \"b.flac\" WAVE\n  TRACK 02 AUDIO\n    INDEX 01 00:00:00\n",
        )?;

        assert!(pieces(&sheet, 44100, 882_000, SHNTOOL).is_err());
        Ok(())
    }
}
