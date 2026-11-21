//! A port of binchunker for Unix (bchunk) 1.2.2 by Heikki Hannikainen.
//!
//! It reads the cue sheet as the original does, not as a [`CueSheet`](crate::cue::CueSheet), and
//! the track files written are those of the original. Unlike the original, a track without an
//! `INDEX` or with an `INDEX` time that is not mins:secs:frames is an error.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, ErrorKind, Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

const SECTOR_LEN: usize = 2352;
const SECTOR_LEN_I64: i64 = 2352;

const FRAMES_PER_SECOND: i32 = 75;
const SECONDS_PER_MINUTE: i32 = 60;

const WAV_HEADER_LEN: usize = 44;
/// Bytes of a WAV file after its RIFF length field and before its samples.
const WAV_RIFF_OVERHEAD: i64 = 36;
const WAV_FORMAT_LEN: u32 = 16;
const WAV_FORMAT_PCM: u16 = 1;
const WAV_CHANNELS: u16 = 2;
const WAV_SAMPLE_RATE: u32 = 44100;
const WAV_BITS_PER_SAMPLE: u16 = 16;
const WAV_BYTES_PER_FRAME: u16 = WAV_CHANNELS * WAV_BITS_PER_SAMPLE / 8;

#[derive(Clone, Copy, Default)]
pub struct Options {
    /// Write all 2352 bytes of each MODE2/2352 sector, instead of the 2048 bytes at offset 24.
    pub raw: bool,
    /// Write the first 2336 bytes of each MODE2/2352 sector, unless `raw`.
    pub psx: bool,
    /// Write audio tracks as WAV instead of headerless CDR.
    pub wav: bool,
    /// Swap the byte order of audio samples.
    pub swap_audio: bool,
    /// Overwrite track files that exist.
    pub force: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct Track {
    num: i32,
    audio: bool,
    extension: &'static str,
    /// Offset of the written data within each sector.
    bstart: usize,
    /// Bytes written from each sector.
    bsize: usize,
    /// The sector of the track's last `INDEX`.
    startsect: Option<i64>,
    /// The sector before the first `INDEX` of the next track, or the last of the image.
    stopsect: Option<i64>,
}

/// Where a track is in the image.
#[derive(Debug, PartialEq, Eq)]
struct Span {
    /// Byte offset of the first sector.
    offset: u64,
    /// Not positive when the track stops before it starts, which leaves its file without data.
    sectors: i64,
}

impl Track {
    fn path(&self, basename: &OsStr) -> PathBuf {
        let mut fname = basename.as_bytes().to_vec();
        fname.extend_from_slice(two_digits(self.num).as_bytes());
        fname.push(b'.');
        fname.extend_from_slice(self.extension.as_bytes());
        PathBuf::from(OsString::from_vec(fname))
    }

    fn span(&self) -> Result<Span> {
        let startsect = self
            .startsect
            .with_context(|| format!("track {} has no INDEX", self.num))?;
        let stopsect = self
            .stopsect
            .with_context(|| format!("track {} has no end", self.num))?;
        let offset = u64::try_from(startsect * SECTOR_LEN_I64)
            .ok()
            .with_context(|| format!("track {} starts before the image", self.num))?;
        Ok(Span {
            offset,
            sectors: stopsect - startsect + 1,
        })
    }
}

/// Writes a file per track `cue` gives the image `bin` and returns their paths, in track order.
///
/// A track's file is `basename`, its two-digit number, and `.iso` for the data of a data track,
/// `.cdr` or `.wav` for an audio track, or `.ugh` for a track of any other mode, written whole.
/// A track runs from its last `INDEX` to before the first `INDEX` of the next track. Unless
/// `options.force`, nothing is written when any track file exists.
pub fn tracks(bin: &Path, cue: &Path, basename: &OsStr, options: &Options) -> Result<Vec<PathBuf>> {
    let image = File::open(bin).with_context(|| format!("opening {}", bin.display()))?;
    let sheet = fs::read(cue).with_context(|| format!("reading {}", cue.display()))?;
    let mut tracks = parse_cue(&sheet, options).with_context(|| cue.display().to_string())?;

    let mut image = BufReader::new(image);
    if let Some(track) = tracks.last_mut() {
        let len = image
            .seek(SeekFrom::End(0))
            .with_context(|| format!("seeking in {}", bin.display()))?;
        let len = i64::try_from(len)
            .with_context(|| format!("{} is too large an image", bin.display()))?;
        track.stopsect = Some((len - 1) / SECTOR_LEN_I64);
    }

    // Checked for every track first: a track is left without a stop by the one after it
    // having no `INDEX`.
    for track in &tracks {
        ensure!(
            track.startsect.is_some(),
            "{}: track {} has no INDEX",
            cue.display(),
            track.num
        );
    }
    let spans = tracks
        .iter()
        .map(Track::span)
        .collect::<Result<Vec<Span>>>()
        .with_context(|| cue.display().to_string())?;
    let paths: Vec<PathBuf> = tracks.iter().map(|track| track.path(basename)).collect();
    if !options.force {
        for path in &paths {
            ensure!(!path.exists(), "{} already exists", path.display());
        }
    }

    for ((track, span), path) in tracks.iter().zip(&spans).zip(&paths) {
        write_track(&mut image, bin, track, span, path, options)?;
    }
    Ok(paths)
}

/// Reads the tracks of a cue sheet.
///
/// The first line is skipped. A track starts at its last `INDEX` and stops one sector before
/// the first `INDEX` of the next track; the last track is left without a stop.
fn parse_cue(cue: &[u8], options: &Options) -> Result<Vec<Track>> {
    if cue.is_empty() {
        bail!("the cue sheet is empty");
    }
    let mut tracks: Vec<Track> = Vec::new();

    for line in cue.split_inclusive(|&byte| byte == b'\n').skip(1) {
        let end = line
            .iter()
            .position(|byte| matches!(byte, b'\r' | b'\n' | 0))
            .unwrap_or(line.len());
        let line = &line[..end];

        if let Some(at) = find(line, b"TRACK") {
            let Some((_, rest)) = split_at_space(&line[at..]) else {
                bail!("no space after TRACK");
            };
            let Some((num, modes)) = split_at_space(rest) else {
                bail!("no space after a track number");
            };
            tracks.push(track_for_mode(atoi(num), modes, options));
        } else if let Some(at) = find(line, b"INDEX") {
            let Some((_, rest)) = split_at_space(&line[at..]) else {
                bail!("no space after INDEX");
            };
            let Some((_, time)) = split_at_space(rest) else {
                bail!("no space after an index number");
            };
            let Some((track, earlier)) = tracks.split_last_mut() else {
                bail!("INDEX before any TRACK");
            };
            let startsect = time2frames(time)?;
            track.startsect = Some(startsect);
            if let Some(previous) = earlier.last_mut()
                && previous.stopsect.is_none()
            {
                previous.stopsect = Some(startsect - 1);
            }
        }
    }

    Ok(tracks)
}

fn track_for_mode(num: i32, modes: &[u8], options: &Options) -> Track {
    let (bstart, bsize, audio, extension) = if modes.eq_ignore_ascii_case(b"MODE1/2352") {
        (16, 2048, false, "iso")
    } else if modes.eq_ignore_ascii_case(b"MODE2/2352") {
        if options.raw {
            (0, 2352, false, "iso")
        } else if options.psx {
            (0, 2336, false, "iso")
        } else {
            (24, 2048, false, "iso")
        }
    } else if modes.eq_ignore_ascii_case(b"MODE2/2336") {
        (16, 2336, false, "iso")
    } else if modes.eq_ignore_ascii_case(b"AUDIO") {
        (0, 2352, true, if options.wav { "wav" } else { "cdr" })
    } else {
        // A mode that is not known is written whole.
        (0, 2352, false, "ugh")
    };
    Track {
        num,
        audio,
        extension,
        bstart,
        bsize,
        startsect: None,
        stopsect: None,
    }
}

fn time2frames(time: &[u8]) -> Result<i64> {
    let mut fields = time.splitn(3, |&byte| byte == b':');
    let (Some(mins), Some(secs), Some(frames)) = (fields.next(), fields.next(), fields.next())
    else {
        bail!(
            "INDEX time {:?} is not mins:secs:frames",
            String::from_utf8_lossy(time)
        );
    };
    let seconds = atoi(mins)
        .wrapping_mul(SECONDS_PER_MINUTE)
        .wrapping_add(atoi(secs));
    Ok(i64::from(
        FRAMES_PER_SECOND
            .wrapping_mul(seconds)
            .wrapping_add(atoi(frames)),
    ))
}

fn atoi(text: &[u8]) -> i32 {
    let mut bytes = text
        .iter()
        .copied()
        .skip_while(|byte| matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        .peekable();
    let negative = match bytes.peek() {
        Some(b'-') => {
            bytes.next();
            true
        }
        Some(b'+') => {
            bytes.next();
            false
        }
        _ => false,
    };
    let mut value: i64 = 0;
    for byte in bytes.take_while(u8::is_ascii_digit) {
        let digit = i64::from(byte - b'0');
        value = value.saturating_mul(10);
        value = if negative {
            value.saturating_sub(digit)
        } else {
            value.saturating_add(digit)
        };
    }
    // glibc's `atoi` truncates the saturated `long` to an `int`.
    value as i32
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn split_at_space(text: &[u8]) -> Option<(&[u8], &[u8])> {
    let space = text.iter().position(|&byte| byte == b' ')?;
    Some((&text[..space], &text[space + 1..]))
}

/// C's `%2.2d`: at least two digits, after any sign.
fn two_digits(num: i32) -> String {
    if num < 0 {
        format!("-{:02}", num.unsigned_abs())
    } else {
        format!("{num:02}")
    }
}

fn write_track(
    image: &mut BufReader<File>,
    bin: &Path,
    track: &Track,
    span: &Span,
    path: &Path,
    options: &Options,
) -> Result<()> {
    let file = if options.force {
        File::create(path)
    } else {
        File::create_new(path)
    }
    .with_context(|| format!("creating {}", path.display()))?;
    let mut file = BufWriter::new(file);

    image
        .seek(SeekFrom::Start(span.offset))
        .with_context(|| format!("seeking in {}", bin.display()))?;

    if track.audio && options.wav {
        file.write_all(&wav_header(span.sectors * track.bsize as i64))
            .with_context(|| format!("writing {}", path.display()))?;
    }

    let mut sector = [0; SECTOR_LEN];
    for _ in 0..span.sectors {
        let whole = read_sector(image, &mut sector)
            .with_context(|| format!("reading {}", bin.display()))?;
        if !whole {
            break;
        }
        let data = &mut sector[track.bstart..track.bstart + track.bsize];
        if track.audio && options.swap_audio {
            for pair in data.as_chunks_mut::<2>().0 {
                pair.swap(0, 1);
            }
        }
        file.write_all(data)
            .with_context(|| format!("writing {}", path.display()))?;
    }

    file.flush()
        .with_context(|| format!("writing {}", path.display()))
}

/// Whether the image had a whole sector left to fill `sector` with.
fn read_sector(image: &mut impl Read, sector: &mut [u8; SECTOR_LEN]) -> io::Result<bool> {
    match image.read_exact(sector) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(error),
    }
}

/// The header of a WAV file holding `data_len` bytes of CD audio. Its lengths wrap to 32 bits
/// as the original's do.
fn wav_header(data_len: i64) -> Vec<u8> {
    let mut header = Vec::with_capacity(WAV_HEADER_LEN);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&((data_len + WAV_RIFF_OVERHEAD) as u32).to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&WAV_FORMAT_LEN.to_le_bytes());
    header.extend_from_slice(&WAV_FORMAT_PCM.to_le_bytes());
    header.extend_from_slice(&WAV_CHANNELS.to_le_bytes());
    header.extend_from_slice(&WAV_SAMPLE_RATE.to_le_bytes());
    header.extend_from_slice(&(WAV_SAMPLE_RATE * u32::from(WAV_BYTES_PER_FRAME)).to_le_bytes());
    header.extend_from_slice(&WAV_BYTES_PER_FRAME.to_le_bytes());
    header.extend_from_slice(&WAV_BITS_PER_SAMPLE.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&(data_len as u32).to_le_bytes());
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Options with the flags whose first letters (`r`, `p`, `w`, `s`, `f`) are in `flags`.
    fn options(flags: &str) -> Options {
        Options {
            raw: flags.contains('r'),
            psx: flags.contains('p'),
            wav: flags.contains('w'),
            swap_audio: flags.contains('s'),
            force: flags.contains('f'),
        }
    }

    /// A sector whose every byte pair is its sector number, then the pair's index.
    fn sector(number: u8) -> Vec<u8> {
        (0..SECTOR_LEN / 2)
            .flat_map(|pair| [number, (pair % 251) as u8])
            .collect()
    }

    const CUE: &str = "FILE \"image.bin\" BINARY\r\n  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\n  TRACK 02 AUDIO\r\n    INDEX 00 00:00:02\r\n    INDEX 01 00:00:03\r\n";

    /// A directory holding `image.cue` and the five sectors of `image.bin`, which it returns.
    fn image() -> Result<(tempfile::TempDir, Vec<u8>)> {
        let dir = tempfile::tempdir()?;
        let image: Vec<u8> = (0..5).flat_map(sector).collect();
        fs::write(dir.path().join("image.bin"), &image)?;
        fs::write(dir.path().join("image.cue"), CUE)?;
        Ok((dir, image))
    }

    /// Extracts the image of `dir` to files that start `track` in it.
    fn extract(dir: &Path, flags: &str) -> Result<Vec<PathBuf>> {
        tracks(
            &dir.join("image.bin"),
            &dir.join("image.cue"),
            dir.join("track").as_os_str(),
            &options(flags),
        )
    }

    /// The error of extracting with `cue` as the cue sheet, with its causes.
    fn extract_error(cue: &str) -> Result<String> {
        let (dir, _) = image()?;
        fs::write(dir.path().join("image.cue"), cue)?;
        let message = match extract(dir.path(), "") {
            Ok(written) => format!("{written:?}"),
            Err(error) => format!("{error:#}"),
        };
        let files = fs::read_dir(dir.path())?.count();
        assert_eq!(files, 2, "nothing is written");
        Ok(message.replace(&format!("{}: ", dir.path().join("image.cue").display()), ""))
    }

    #[test]
    fn time2frames_counts_75_frames_a_second() -> Result<()> {
        assert_eq!(time2frames(b"00:00:00")?, 0);
        assert_eq!(time2frames(b"00:02:00")?, 150);
        assert_eq!(time2frames(b"01:02:03")?, 75 * 62 + 3);
        assert_eq!(time2frames(b"1:2:3:4")?, 75 * 62 + 3);
        assert!(time2frames(b"00:02").is_err());
        assert!(time2frames(b"").is_err());
        Ok(())
    }

    #[test]
    fn atoi_stops_at_the_first_non_digit() {
        assert_eq!(atoi(b"01"), 1);
        assert_eq!(atoi(b"  -12x"), -12);
        assert_eq!(atoi(b"+7"), 7);
        assert_eq!(atoi(b"x1"), 0);
        assert_eq!(atoi(b""), 0);
        assert_eq!(atoi(b"99999999999999999999"), -1);
        assert_eq!(atoi(b"-99999999999999999999"), 0);
    }

    #[test]
    fn two_digits_pads_after_the_sign() {
        assert_eq!(two_digits(1), "01");
        assert_eq!(two_digits(100), "100");
        assert_eq!(two_digits(-1), "-01");
    }

    #[test]
    fn parse_cue_takes_the_last_index_and_stops_before_the_next_first_index() -> Result<()> {
        let tracks = parse_cue(CUE.as_bytes(), &options(""))?;
        assert_eq!(
            tracks,
            [
                Track {
                    num: 1,
                    audio: false,
                    extension: "iso",
                    bstart: 24,
                    bsize: 2048,
                    startsect: Some(0),
                    stopsect: Some(1),
                },
                Track {
                    num: 2,
                    audio: true,
                    extension: "cdr",
                    bstart: 0,
                    bsize: 2352,
                    startsect: Some(3),
                    stopsect: None,
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn parse_cue_writes_an_unknown_mode_whole() -> Result<()> {
        let tracks = parse_cue(b"\nTRACK 03 CDG \n", &options(""))?;
        assert_eq!(
            (tracks[0].bstart, tracks[0].bsize, tracks[0].extension),
            (0, 2352, "ugh")
        );
        Ok(())
    }

    #[test]
    fn parse_cue_rejects_malformed_lines() {
        let message = |cue: &[u8]| match parse_cue(cue, &options("")) {
            Ok(tracks) => format!("{tracks:?}"),
            Err(error) => error.to_string(),
        };
        assert_eq!(message(b""), "the cue sheet is empty");
        assert_eq!(message(b"\nTRACK\n"), "no space after TRACK");
        assert_eq!(message(b"\nTRACK 01\n"), "no space after a track number");
        assert_eq!(
            message(b"\nTRACK 01 AUDIO\nINDEX 01\n"),
            "no space after an index number"
        );
        assert_eq!(message(b"\nINDEX 01 00:00:00\n"), "INDEX before any TRACK");
        assert_eq!(
            message(b"\nTRACK 01 AUDIO\nINDEX 01 00:00\n"),
            "INDEX time \"00:00\" is not mins:secs:frames"
        );
    }

    #[test]
    fn a_track_that_cannot_be_placed_stops_everything() -> Result<()> {
        assert_eq!(
            extract_error("x\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\n")?,
            "track 2 has no INDEX"
        );
        assert_eq!(
            extract_error(
                "x\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nTRACK 03 AUDIO\nINDEX 01 00:00:02\n"
            )?,
            "track 2 has no INDEX"
        );
        assert_eq!(
            extract_error("x\nTRACK 01 AUDIO\nINDEX 01 00:00:-1\n")?,
            "track 1 starts before the image"
        );
        assert_eq!(
            extract_error("x\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 0:0\n")?,
            "INDEX time \"0:0\" is not mins:secs:frames"
        );
        Ok(())
    }

    #[test]
    fn track_mode_follows_the_options() {
        let layout = |flags: &str, modes: &[u8]| {
            let track = track_for_mode(1, modes, &options(flags));
            (track.bstart, track.bsize, track.extension)
        };
        assert_eq!(layout("", b"mode1/2352"), (16, 2048, "iso"));
        assert_eq!(layout("", b"MODE2/2352"), (24, 2048, "iso"));
        assert_eq!(layout("r", b"MODE2/2352"), (0, 2352, "iso"));
        assert_eq!(layout("p", b"MODE2/2352"), (0, 2336, "iso"));
        assert_eq!(layout("rp", b"MODE2/2352"), (0, 2352, "iso"));
        assert_eq!(layout("", b"MODE2/2336"), (16, 2336, "iso"));
        assert_eq!(layout("", b"AUDIO"), (0, 2352, "cdr"));
        assert_eq!(layout("w", b"AUDIO"), (0, 2352, "wav"));
    }

    #[test]
    fn writes_the_data_of_data_tracks_and_the_samples_of_audio_tracks() -> Result<()> {
        let (dir, image) = image()?;

        let written = extract(dir.path(), "ws")?;

        let iso_path = dir.path().join("track01.iso");
        let wav_path = dir.path().join("track02.wav");
        assert_eq!(written, [iso_path.clone(), wav_path.clone()]);

        let expected: Vec<u8> = image
            .chunks(SECTOR_LEN)
            .take(2)
            .flat_map(|sector| &sector[24..24 + 2048])
            .copied()
            .collect();
        assert_eq!(fs::read(iso_path)?, expected);

        let wav = fs::read(wav_path)?;
        let audio = &image[3 * SECTOR_LEN..];
        let swapped: Vec<u8> = audio
            .chunks(2)
            .flat_map(|pair| [pair[1], pair[0]])
            .collect();
        let mut expected = b"RIFF".to_vec();
        expected.extend_from_slice(&(audio.len() as u32 + 36).to_le_bytes());
        expected.extend_from_slice(b"WAVEfmt ");
        expected.extend_from_slice(&[16, 0, 0, 0, 1, 0, 2, 0]);
        expected.extend_from_slice(&44100_u32.to_le_bytes());
        expected.extend_from_slice(&176_400_u32.to_le_bytes());
        expected.extend_from_slice(&[4, 0, 16, 0]);
        expected.extend_from_slice(b"data");
        expected.extend_from_slice(&(audio.len() as u32).to_le_bytes());
        expected.extend_from_slice(&swapped);
        assert_eq!(wav, expected);
        Ok(())
    }

    #[test]
    fn an_existing_track_file_stops_everything_without_force() -> Result<()> {
        let (dir, _) = image()?;
        let existing = dir.path().join("track02.cdr");
        fs::write(&existing, b"old")?;

        let error = extract(dir.path(), "").map(drop).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{} already exists", existing.display())
        );
        assert!(!dir.path().join("track01.iso").exists());
        assert_eq!(fs::read(&existing)?, b"old");

        extract(dir.path(), "f")?;
        assert_eq!(fs::read(&existing)?.len(), 2 * SECTOR_LEN);
        Ok(())
    }

    #[test]
    fn missing_image_is_an_error() -> Result<()> {
        let (dir, _) = image()?;
        fs::remove_file(dir.path().join("image.bin"))?;
        assert!(extract(dir.path(), "").is_err());
        Ok(())
    }
}
