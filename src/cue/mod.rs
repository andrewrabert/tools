use std::ffi::OsStr;
use std::fmt;
use std::path::Path;

use winnow::Parser;

use crate::chardet::{self, MalformedError};
use winnow::combinator::{alt, preceded, repeat};
use winnow::error::{EmptyError, ParseError};
use winnow::stream::Stream;
use winnow::token::{take_till, take_while};

pub const EXTENSION: &str = "cue";

#[derive(Clone, Copy, PartialEq)]
pub enum Command {
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
    pub fn as_str(self) -> &'static str {
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

#[derive(Clone, PartialEq)]
pub struct Parameter(String);

impl Parameter {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Parameter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() || self.0.contains(|c: char| c.is_whitespace() || c == '"') {
            write!(f, "\"{}\"", self.0)
        } else {
            f.write_str(&self.0)
        }
    }
}

pub struct CueSheet {
    commands: Vec<(Command, Vec<Parameter>)>,
    files: Vec<File>,
    tracks: Vec<Track>,
}

impl CueSheet {
    pub fn new(data: &[u8]) -> Result<Self, CueSheetError> {
        let text = chardet::to_utf8(data).map_err(CueSheetError::MalformedEncoding)?;
        if text.as_bytes() != data {
            return Err(CueSheetError::CharacterEncoding(text.into_owned()));
        }
        let mut commands = Vec::new();
        let mut files = Vec::new();
        let mut tracks = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let (name, parameters): (&str, Vec<&str>) =
                (word, repeat(0.., preceded(whitespace, argument)))
                    .parse(line)
                    .map_err(|_: ParseError<&str, EmptyError>| CueSheetError::UnmatchedQuote)?;
            let command = Command::try_from(name)?;
            let parameters: Vec<Parameter> = match command {
                Command::File => {
                    let file: File = parse(&parameters, raw)?;
                    let parameters = file.tokens().map(Parameter).into();
                    files.push(file);
                    parameters
                }
                Command::Track => {
                    let track: Track = parse(&parameters, raw)?;
                    let parameters = track.tokens().map(Parameter).into();
                    tracks.push(track);
                    parameters
                }
                Command::Index => {
                    let Some(track) = tracks.last_mut() else {
                        return Err(CueSheetError::IndexOutsideTrack(raw.to_owned()));
                    };
                    let index: Index = parse(&parameters, raw)?;
                    let parameters = index.tokens().map(Parameter).into();
                    track.indexes.push(index);
                    parameters
                }
                Command::Rem if parameters.first() == Some(&"SESSION") => {
                    let session: Session = parse(&parameters, raw)?;
                    session.tokens().map(Parameter).into()
                }
                Command::Rem
                | Command::Catalog
                | Command::Cdtextfile
                | Command::Flags
                | Command::Isrc
                | Command::Performer
                | Command::Postgap
                | Command::Pregap
                | Command::Songwriter
                | Command::Title => parameters
                    .into_iter()
                    .map(|parameter| Parameter(parameter.to_owned()))
                    .collect(),
            };
            commands.push((command, parameters));
        }
        if tracks.is_empty() {
            return Err(CueSheetError::NoTracks);
        }
        Ok(Self {
            commands,
            files,
            tracks,
        })
    }

    pub fn commands(&self) -> &[(Command, Vec<Parameter>)] {
        &self.commands
    }

    pub fn files(&self) -> &[File] {
        &self.files
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }
}

/// The sheet's lines, indented, one space between tokens, each line ending in `\n`. `FILE` is
/// indented 0 and `TRACK` 2. Any other command is 0 before the first `TRACK` and 4 after it.
/// `REM` takes the indent of the next line that is not `REM`, or 0 at the end of the sheet.
impl fmt::Display for CueSheet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut in_track = false;
        let mut indents: Vec<Option<usize>> = self
            .commands
            .iter()
            .map(|(command, _)| match command {
                Command::File => Some(0),
                Command::Track => {
                    in_track = true;
                    Some(2)
                }
                Command::Rem => None,
                Command::Catalog
                | Command::Cdtextfile
                | Command::Flags
                | Command::Index
                | Command::Isrc
                | Command::Performer
                | Command::Postgap
                | Command::Pregap
                | Command::Songwriter
                | Command::Title => Some(if in_track { 4 } else { 0 }),
            })
            .collect();
        let mut next = 0;
        for indent in indents.iter_mut().rev() {
            next = *indent.get_or_insert(next);
        }
        for ((command, parameters), indent) in
            self.commands.iter().zip(indents.into_iter().flatten())
        {
            write!(f, "{:indent$}{}", "", command.as_str())?;
            for parameter in parameters {
                write!(f, " {parameter}")?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
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

trait Tokens<const N: usize>: Sized {
    fn from_tokens(tokens: [&str; N]) -> Result<Self, CueSheetError>;
    fn tokens(&self) -> [String; N];
}

fn parse<T: Tokens<N>, const N: usize>(parameters: &[&str], raw: &str) -> Result<T, CueSheetError> {
    let tokens: [&str; N] = parameters
        .try_into()
        .map_err(|_| CueSheetError::ParseCommand(raw.to_owned()))?;
    T::from_tokens(tokens)
}

pub struct File {
    filename: String,
    filetype: Filetype,
}

impl File {
    pub fn filename(&self) -> &str {
        &self.filename
    }
}

impl Tokens<2> for File {
    fn from_tokens([filename, filetype]: [&str; 2]) -> Result<Self, CueSheetError> {
        if Path::new(filename).file_name() != Some(OsStr::new(filename)) {
            return Err(CueSheetError::Filename(filename.to_owned()));
        }
        Ok(File {
            filename: filename.to_owned(),
            filetype: Filetype::from(filetype),
        })
    }

    fn tokens(&self) -> [String; 2] {
        [self.filename.clone(), self.filetype.to_string()]
    }
}

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

impl fmt::Display for Filetype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Filetype::Binary => "BINARY",
            Filetype::Motorola => "MOTOROLA",
            Filetype::Aiff => "AIFF",
            Filetype::Wave => "WAVE",
            Filetype::Mp3 => "MP3",
            Filetype::Other(text) => text,
        })
    }
}

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

struct SessionNumber(u8);

impl TryFrom<&str> for SessionNumber {
    type Error = CueSheetError;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        match text.parse() {
            Ok(number @ 1..=99) => Ok(SessionNumber(number)),
            _ => Err(CueSheetError::SessionNumber(text.to_owned())),
        }
    }
}

impl fmt::Display for SessionNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}", self.0)
    }
}

struct Session {
    number: SessionNumber,
}

impl Tokens<2> for Session {
    fn from_tokens([_, number]: [&str; 2]) -> Result<Self, CueSheetError> {
        Ok(Session {
            number: SessionNumber::try_from(number)?,
        })
    }

    fn tokens(&self) -> [String; 2] {
        ["SESSION".to_owned(), self.number.to_string()]
    }
}

impl Track {
    pub fn datatype(&self) -> &Datatype {
        &self.datatype
    }
}

impl Tokens<2> for Track {
    fn from_tokens([number, datatype]: [&str; 2]) -> Result<Self, CueSheetError> {
        Ok(Track {
            number: TrackNumber::try_from(number)?,
            datatype: Datatype::try_from(datatype)?,
            indexes: Vec::new(),
        })
    }

    fn tokens(&self) -> [String; 2] {
        [self.number.to_string(), self.datatype.to_string()]
    }
}

pub struct Index {
    number: IndexNumber,
    time: Time,
}

impl Tokens<2> for Index {
    fn from_tokens([number, time]: [&str; 2]) -> Result<Self, CueSheetError> {
        Ok(Index {
            number: IndexNumber::try_from(number)?,
            time: Time::try_from(time)?,
        })
    }

    fn tokens(&self) -> [String; 2] {
        [self.number.to_string(), self.time.to_string()]
    }
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
    ParseCommand(String),
    TrackNumber(String),
    SessionNumber(String),
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
            CueSheetError::ParseCommand(line) => write!(f, "{line:?}"),
            CueSheetError::TrackNumber(number) => {
                write!(f, "{number:?} is not a track number from 1 to 99")
            }
            CueSheetError::SessionNumber(number) => {
                write!(f, "{number:?} is not a session number from 1 to 99")
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
            CueSheetError::Filename(filename) => {
                write!(
                    f,
                    "{filename:?} must name a file in the cue sheet's directory"
                )
            }
        }
    }
}

impl std::error::Error for CueSheetError {}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand-written cue sheets. Each invalid one is a minimal sheet with a single defect.
    macro_rules! fixture {
        ($name:literal) => {
            include_bytes!(concat!("testdata/", $name, ".cue"))
        };
    }

    fn sheet(data: &[u8]) -> CueSheet {
        CueSheet::new(data).expect("valid cue sheet")
    }

    fn formatted(data: &[u8]) -> String {
        sheet(data).to_string()
    }

    fn text(data: &'static [u8]) -> &'static str {
        std::str::from_utf8(data).expect("UTF-8 fixture")
    }

    fn canonical_text() -> &'static str {
        text(fixture!("canonical"))
    }

    fn error(data: &[u8]) -> CueSheetError {
        CueSheet::new(data).err().expect("invalid cue sheet")
    }

    #[test]
    fn canonical() {
        let sheet = sheet(fixture!("canonical"));
        assert_eq!(sheet.to_string(), canonical_text());

        let numbers: Vec<u8> = sheet.tracks.iter().map(|track| track.number.0).collect();
        assert_eq!(numbers, [1, 2]);
        assert!(
            sheet
                .tracks
                .iter()
                .all(|track| matches!(track.datatype, Datatype::Audio))
        );

        let indexes = &sheet.tracks[1].indexes;
        assert_eq!(indexes.len(), 2);
        assert_eq!(indexes[0].number.0, 0);
        assert_eq!(
            (
                indexes[0].time.minutes,
                indexes[0].time.seconds,
                indexes[0].time.frames
            ),
            (3, 58, 70)
        );
        assert_eq!(indexes[1].number.0, 1);
        assert_eq!(
            (
                indexes[1].time.minutes,
                indexes[1].time.seconds,
                indexes[1].time.frames
            ),
            (4, 0, 0)
        );
    }

    // canonical with bad indentation, extra spaces, a blank line, unpadded track numbers, and
    // needlessly quoted tokens.
    #[test]
    fn unformatted() {
        assert_eq!(formatted(fixture!("unformatted")), canonical_text());
    }

    #[test]
    fn crlf_line_endings() {
        assert_eq!(formatted(fixture!("crlf_line_endings")), canonical_text());
    }

    #[test]
    fn multiple_files() {
        let data = fixture!("multiple_files");
        let sheet = sheet(data);
        assert_eq!(sheet.to_string(), text(data));

        assert_eq!(sheet.files.len(), 3);
        assert_eq!(sheet.tracks.len(), 4);
        assert!(matches!(sheet.tracks[0].datatype, Datatype::Mode1_2352));
        assert!(matches!(sheet.tracks[1].datatype, Datatype::Audio));
        assert!(matches!(sheet.tracks[2].datatype, Datatype::Mode2_2352));
        assert!(matches!(sheet.tracks[3].datatype, Datatype::Cdi2352));
        assert_eq!(sheet.tracks[2].indexes.len(), 2);
    }

    // A REM takes the indent of the next non-REM line.
    #[test]
    fn rem_in_track() {
        let data = fixture!("rem_in_track");
        assert_eq!(formatted(data), text(data));
    }

    // An empty token, a title starting with a tab, a title with an inner quote, and unknown
    // filetypes.
    #[test]
    fn quoting() {
        let data = fixture!("quoting");
        let sheet = sheet(data);
        assert_eq!(sheet.to_string(), text(data));

        assert_eq!(sheet.files.len(), 2);
        assert!(matches!(&sheet.files[0].filetype, Filetype::Other(name) if name == "APE"));
        assert!(matches!(&sheet.files[1].filetype, Filetype::Other(name) if name == "wave"));
    }

    #[test]
    fn no_trailing_newline() {
        assert_eq!(formatted(fixture!("no_trailing_newline")), canonical_text());
    }

    #[test]
    fn byte_order_mark() {
        assert!(matches!(
            error(fixture!("byte_order_mark")),
            CueSheetError::CharacterEncoding(_)
        ));
    }

    // The title is encoded as Latin-1.
    #[test]
    fn latin1() {
        assert!(matches!(
            error(fixture!("latin1")),
            CueSheetError::CharacterEncoding(_)
        ));
    }

    #[test]
    fn index_frames_75() {
        assert!(matches!(
            error(fixture!("index_frames_75")),
            CueSheetError::Time(_)
        ));
    }

    #[test]
    fn index_seconds_60() {
        assert!(matches!(
            error(fixture!("index_seconds_60")),
            CueSheetError::Time(_)
        ));
    }

    #[test]
    fn index_minutes_one_digit() {
        assert!(matches!(
            error(fixture!("index_minutes_one_digit")),
            CueSheetError::Time(_)
        ));
    }

    #[test]
    fn index_number_three_digits() {
        assert!(matches!(
            error(fixture!("index_number_three_digits")),
            CueSheetError::IndexNumber(_)
        ));
    }

    // The INDEX has a number but no time.
    #[test]
    fn index_one_argument() {
        assert!(matches!(
            error(fixture!("index_one_argument")),
            CueSheetError::ParseCommand(_)
        ));
    }

    #[test]
    fn index_before_track() {
        assert!(matches!(
            error(fixture!("index_before_track")),
            CueSheetError::IndexOutsideTrack(_)
        ));
    }

    #[test]
    fn track_number_zero() {
        assert!(matches!(
            error(fixture!("track_number_zero")),
            CueSheetError::TrackNumber(_)
        ));
    }

    #[test]
    fn track_number_100() {
        assert!(matches!(
            error(fixture!("track_number_100")),
            CueSheetError::TrackNumber(_)
        ));
    }

    #[test]
    fn unknown_datatype() {
        assert!(matches!(
            error(fixture!("unknown_datatype")),
            CueSheetError::Datatype(_)
        ));
    }

    #[test]
    fn no_tracks() {
        assert!(matches!(
            error(fixture!("no_tracks")),
            CueSheetError::NoTracks
        ));
    }

    // A `;` comment line, which is not a cue sheet command.
    #[test]
    fn unknown_command() {
        assert!(matches!(
            error(fixture!("unknown_command")),
            CueSheetError::UnknownCommand(_)
        ));
    }

    #[test]
    fn unmatched_quote() {
        assert!(matches!(
            error(fixture!("unmatched_quote")),
            CueSheetError::UnmatchedQuote
        ));
    }

    #[test]
    fn filename_with_directory() {
        assert!(matches!(
            error(fixture!("filename_with_directory")),
            CueSheetError::Filename(_)
        ));
    }
}
