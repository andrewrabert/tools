use std::borrow::Cow;
use std::ffi::OsStr;
use std::fmt;
use std::iter;
use std::path::Path;

use winnow::Parser;

use crate::chardet::{self, MalformedError};
use winnow::combinator::{alt, preceded, repeat};
use winnow::error::{EmptyError, ParseError};
use winnow::stream::Stream;
use winnow::token::{take_till, take_while};

pub const EXTENSION: &str = "cue";

#[derive(Clone, Copy, PartialEq)]
enum Command {
    Catalog,
    Cdtextfile,
    File,
    Flags,
    Index,
    Isrc,
    Performer,
    Postgap,
    Pregap,
    Rem,
    Songwriter,
    Title,
    Track,
}

impl TryFrom<&str> for Command {
    type Error = CueSheetError;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        match text {
            "CATALOG" => Ok(Command::Catalog),
            "CDTEXTFILE" => Ok(Command::Cdtextfile),
            "FILE" => Ok(Command::File),
            "FLAGS" => Ok(Command::Flags),
            "INDEX" => Ok(Command::Index),
            "ISRC" => Ok(Command::Isrc),
            "PERFORMER" => Ok(Command::Performer),
            "POSTGAP" => Ok(Command::Postgap),
            "PREGAP" => Ok(Command::Pregap),
            "REM" => Ok(Command::Rem),
            "SONGWRITER" => Ok(Command::Songwriter),
            "TITLE" => Ok(Command::Title),
            "TRACK" => Ok(Command::Track),
            other => Err(CueSheetError::UnknownCommand(other.to_owned())),
        }
    }
}

impl Command {
    fn as_str(self) -> &'static str {
        match self {
            Command::Catalog => "CATALOG",
            Command::Cdtextfile => "CDTEXTFILE",
            Command::File => "FILE",
            Command::Flags => "FLAGS",
            Command::Index => "INDEX",
            Command::Isrc => "ISRC",
            Command::Performer => "PERFORMER",
            Command::Postgap => "POSTGAP",
            Command::Pregap => "PREGAP",
            Command::Rem => "REM",
            Command::Songwriter => "SONGWRITER",
            Command::Title => "TITLE",
            Command::Track => "TRACK",
        }
    }
}

#[derive(PartialEq)]
pub struct Line {
    command: Command,
    arguments: Vec<String>,
}

impl Line {
    pub fn tokens(&self) -> impl Iterator<Item = &str> {
        iter::once(self.command.as_str()).chain(self.arguments.iter().map(String::as_str))
    }
}

pub struct CueSheet {
    lines: Vec<Line>,
    files: Vec<File>,
    tracks: Vec<Track>,
}

impl CueSheet {
    pub fn new(data: &[u8]) -> Result<Self, CueSheetError> {
        let text = chardet::to_utf8(data).map_err(CueSheetError::MalformedEncoding)?;
        if text.as_bytes() != data {
            return Err(CueSheetError::CharacterEncoding(text.into_owned()));
        }
        let mut lines = Vec::new();
        let mut files = Vec::new();
        let mut tracks = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let (command, arguments) = command_and_arguments(line)?;
            match command {
                Command::File => {
                    let &[filename, filetype] = arguments.as_slice() else {
                        return Err(CueSheetError::ParseFileCommand(raw.to_owned()));
                    };
                    if Path::new(filename).file_name() != Some(OsStr::new(filename)) {
                        return Err(CueSheetError::Filename(raw.to_owned()));
                    }
                    files.push(File {
                        filename: filename.to_owned(),
                        filetype: Filetype::from(filetype),
                    });
                }
                Command::Track => {
                    let &[number, datatype] = arguments.as_slice() else {
                        return Err(CueSheetError::ParseTrackCommand(raw.to_owned()));
                    };
                    tracks.push(Track {
                        number: TrackNumber::try_from(number)?,
                        datatype: Datatype::try_from(datatype)?,
                        indexes: Vec::new(),
                    });
                }
                Command::Index => {
                    let &[number, time] = arguments.as_slice() else {
                        return Err(CueSheetError::ParseIndexCommand(raw.to_owned()));
                    };
                    let Some(track) = tracks.last_mut() else {
                        return Err(CueSheetError::IndexOutsideTrack(raw.to_owned()));
                    };
                    track.indexes.push(Index {
                        number: IndexNumber::try_from(number)?,
                        time: Time::try_from(time)?,
                    });
                }
                Command::Catalog
                | Command::Cdtextfile
                | Command::Flags
                | Command::Isrc
                | Command::Performer
                | Command::Postgap
                | Command::Pregap
                | Command::Rem
                | Command::Songwriter
                | Command::Title => {}
            }
            lines.push(Line {
                command,
                arguments: arguments.into_iter().map(str::to_owned).collect(),
            });
        }
        if tracks.is_empty() {
            return Err(CueSheetError::NoTracks);
        }
        Ok(Self {
            lines,
            files,
            tracks,
        })
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    pub fn files(&self) -> &[File] {
        &self.files
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// The number of spaces before each line. `FILE` is 0 and `TRACK` 2. Any other command is 0
    /// before the first `TRACK` and 4 after it. `REM` takes the indent of the next line that is
    /// not `REM`, or 0 at the end of the sheet.
    fn indents(&self) -> Vec<usize> {
        let mut in_track = false;
        let mut indents: Vec<(bool, usize)> = self
            .lines
            .iter()
            .map(|line| match line.command {
                Command::File => (false, 0),
                Command::Track => {
                    in_track = true;
                    (false, 2)
                }
                Command::Rem => (true, 0),
                Command::Catalog
                | Command::Cdtextfile
                | Command::Flags
                | Command::Index
                | Command::Isrc
                | Command::Performer
                | Command::Postgap
                | Command::Pregap
                | Command::Songwriter
                | Command::Title => (false, if in_track { 4 } else { 0 }),
            })
            .collect();
        let mut next = 0;
        for (is_rem, indent) in indents.iter_mut().rev() {
            if *is_rem {
                *indent = next;
            } else {
                next = *indent;
            }
        }
        indents.into_iter().map(|(_, indent)| indent).collect()
    }
}

/// The sheet's lines, indented, one space between tokens, each line ending in `\n`.
impl fmt::Display for CueSheet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (line, indent) in self.lines.iter().zip(self.indents()) {
            write!(f, "{:indent$}{}", "", line.command.as_str())?;
            for argument in &line.arguments {
                write!(f, " {}", quote(argument))?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

fn quote(token: &str) -> Cow<'_, str> {
    if token.is_empty() || token.contains(|c: char| c.is_whitespace() || c == '"') {
        Cow::Owned(format!("\"{token}\""))
    } else {
        Cow::Borrowed(token)
    }
}

fn command_and_arguments(line: &str) -> Result<(Command, Vec<&str>), CueSheetError> {
    let (command, arguments): (&str, Vec<&str>) =
        (word, repeat(0.., preceded(whitespace, argument)))
            .parse(line)
            .map_err(|_: ParseError<&str, EmptyError>| CueSheetError::UnmatchedQuote)?;
    Ok((Command::try_from(command)?, arguments))
}

fn word<'i>(input: &mut &'i str) -> winnow::Result<&'i str, EmptyError> {
    take_till(1.., char::is_whitespace).parse_next(input)
}

fn whitespace<'i>(input: &mut &'i str) -> winnow::Result<&'i str, EmptyError> {
    take_while(0.., char::is_whitespace).parse_next(input)
}

fn argument<'i>(input: &mut &'i str) -> winnow::Result<&'i str, EmptyError> {
    alt((quoted, bare)).parse_next(input)
}

fn quoted<'i>(input: &mut &'i str) -> winnow::Result<&'i str, EmptyError> {
    '"'.parse_next(input)?;
    let end = input.rfind('"').ok_or(EmptyError)?;
    let value = input.next_slice(end);
    '"'.parse_next(input)?;
    Ok(value)
}

fn bare<'i>(input: &mut &'i str) -> winnow::Result<&'i str, EmptyError> {
    take_till(1.., |c: char| c.is_whitespace() || c == '"').parse_next(input)
}

#[expect(dead_code, reason = "validated CUE spec: FILE fields")]
pub struct File {
    filename: String,
    filetype: Filetype,
}

impl File {
    pub fn filename(&self) -> &str {
        &self.filename
    }
}

#[expect(dead_code, reason = "validated CUE spec: FILE types")]
pub enum Filetype {
    Binary,
    Motorola,
    Aiff,
    Wave,
    Mp3,
    Other(String),
}

impl From<&str> for Filetype {
    fn from(text: &str) -> Self {
        match text {
            "BINARY" => Filetype::Binary,
            "MOTOROLA" => Filetype::Motorola,
            "AIFF" => Filetype::Aiff,
            "WAVE" => Filetype::Wave,
            "MP3" => Filetype::Mp3,
            other => Filetype::Other(other.to_owned()),
        }
    }
}

#[expect(dead_code, reason = "validated CUE spec: TRACK fields")]
pub struct Track {
    number: TrackNumber,
    datatype: Datatype,
    indexes: Vec<Index>,
}

struct TrackNumber(u8);

impl TryFrom<&str> for TrackNumber {
    type Error = CueSheetError;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        match text.parse() {
            Ok(number @ 1..=99) => Ok(TrackNumber(number)),
            _ => Err(CueSheetError::TrackNumber(text.to_owned())),
        }
    }
}

impl fmt::Display for TrackNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}", self.0)
    }
}

impl Track {
    pub fn datatype(&self) -> &Datatype {
        &self.datatype
    }
}

#[expect(dead_code, reason = "validated CUE spec: INDEX fields")]
pub struct Index {
    number: IndexNumber,
    time: Time,
}

struct IndexNumber(u8);

impl TryFrom<&str> for IndexNumber {
    type Error = CueSheetError;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        two_digits
            .parse(text)
            .map(IndexNumber)
            .map_err(|_: ParseError<&str, EmptyError>| CueSheetError::IndexNumber(text.to_owned()))
    }
}

impl fmt::Display for IndexNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}", self.0)
    }
}

struct Time {
    minutes: u8,
    seconds: u8,
    frames: u8,
}

impl TryFrom<&str> for Time {
    type Error = CueSheetError;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        let (minutes, seconds, frames) = (
            two_digits,
            preceded(':', two_digits.verify(|seconds: &u8| *seconds <= 59)),
            preceded(':', two_digits.verify(|frames: &u8| *frames <= 74)),
        )
            .parse(text)
            .map_err(|_: ParseError<&str, EmptyError>| CueSheetError::Time(text.to_owned()))?;
        Ok(Time {
            minutes,
            seconds,
            frames,
        })
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02}:{:02}:{:02}",
            self.minutes, self.seconds, self.frames
        )
    }
}

fn two_digits(input: &mut &str) -> winnow::Result<u8, EmptyError> {
    take_while(2, |c: char| c.is_ascii_digit())
        .parse_to()
        .parse_next(input)
}

pub enum Datatype {
    Audio,
    Cdg,
    Mode1_2048,
    Mode1_2352,
    Mode2_2336,
    Mode2_2352,
    Cdi2336,
    Cdi2352,
}

impl TryFrom<&str> for Datatype {
    type Error = CueSheetError;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        match text {
            "AUDIO" => Ok(Datatype::Audio),
            "CDG" => Ok(Datatype::Cdg),
            "MODE1/2048" => Ok(Datatype::Mode1_2048),
            "MODE1/2352" => Ok(Datatype::Mode1_2352),
            "MODE2/2336" => Ok(Datatype::Mode2_2336),
            "MODE2/2352" => Ok(Datatype::Mode2_2352),
            "CDI/2336" => Ok(Datatype::Cdi2336),
            "CDI/2352" => Ok(Datatype::Cdi2352),
            other => Err(CueSheetError::Datatype(other.to_owned())),
        }
    }
}

impl fmt::Display for Datatype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Datatype::Audio => "AUDIO",
            Datatype::Cdg => "CDG",
            Datatype::Mode1_2048 => "MODE1/2048",
            Datatype::Mode1_2352 => "MODE1/2352",
            Datatype::Mode2_2336 => "MODE2/2336",
            Datatype::Mode2_2352 => "MODE2/2352",
            Datatype::Cdi2336 => "CDI/2336",
            Datatype::Cdi2352 => "CDI/2352",
        })
    }
}

#[derive(Debug)]
pub enum CueSheetError {
    CharacterEncoding(String),
    MalformedEncoding(MalformedError),
    NoTracks,
    UnknownCommand(String),
    UnmatchedQuote,
    ParseFileCommand(String),
    ParseTrackCommand(String),
    ParseIndexCommand(String),
    TrackNumber(String),
    Datatype(String),
    IndexNumber(String),
    Time(String),
    IndexOutsideTrack(String),
    Filename(String),
}

impl fmt::Display for CueSheetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CueSheetError::CharacterEncoding(_) => {
                f.write_str("the cue sheet is not UTF-8 without a byte-order mark")
            }
            CueSheetError::MalformedEncoding(error) => fmt::Display::fmt(error, f),
            CueSheetError::NoTracks => f.write_str("the cue sheet has no tracks"),
            CueSheetError::UnknownCommand(command) => {
                write!(f, "{command:?} is not a command")
            }
            CueSheetError::UnmatchedQuote => f.write_str("a quotation mark has no partner"),
            CueSheetError::ParseFileCommand(line)
            | CueSheetError::ParseTrackCommand(line)
            | CueSheetError::ParseIndexCommand(line) => write!(f, "{line:?}"),
            CueSheetError::TrackNumber(number) => {
                write!(f, "{number:?} is not a track number from 1 to 99")
            }
            CueSheetError::Datatype(datatype) => {
                write!(f, "{datatype:?} is not a track datatype")
            }
            CueSheetError::IndexNumber(number) => {
                write!(f, "{number:?} is not an index number from 00 to 99")
            }
            CueSheetError::Time(time) => write!(
                f,
                "{time:?} is not a time mm:ss:ff with seconds up to 59 and frames up to 74"
            ),
            CueSheetError::IndexOutsideTrack(line) => {
                write!(f, "{line:?} comes before any TRACK")
            }
            CueSheetError::Filename(line) => {
                write!(f, "{line:?} must name a file in the cue sheet's directory")
            }
        }
    }
}

impl std::error::Error for CueSheetError {}
