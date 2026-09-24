use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, IsTerminal, Read, Seek, SeekFrom, Write};
use std::os::fd::AsFd;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args as ClapArgs, ValueEnum};

use crate::source::Source;
use crate::tools::Tool;

const MAX_COLS: usize = 256;
const LOWER: &[u8; 16] = b"0123456789abcdef";
const UPPER: &[u8; 16] = b"0123456789ABCDEF";

#[derive(ClapArgs)]
#[command(about = "Hex dump data in xxd format, or convert a hex dump back to binary")]
pub struct Hexd {
    /// Replace runs of zero lines with a single '*'
    #[arg(short, long)]
    autoskip: bool,
    /// Dump bits instead of hex
    #[arg(short, long, conflicts_with_all = ["little_endian", "plain"])]
    bits: bool,
    /// Octets per line [default: 16, -b: 6, -i: 12, -p: 30]
    #[arg(short, long, value_parser = parse_size)]
    cols: Option<usize>,
    /// Show offsets in decimal
    #[arg(short, long)]
    decimal: bool,
    /// Dump groups in little-endian order
    #[arg(short = 'e', long, conflicts_with_all = ["include", "plain", "revert"])]
    little_endian: bool,
    /// Octets per group [default: 2, -b: 1, -e: 4]
    #[arg(short, long, value_parser = parse_size)]
    groupsize: Option<usize>,
    /// Output a C include array
    #[arg(short, long, conflicts_with_all = ["plain", "revert"])]
    include: bool,
    /// Stop after this many octets
    #[arg(short, long, value_parser = parse_number)]
    len: Option<u64>,
    /// Add this to the displayed offsets
    #[arg(short, long, value_parser = parse_offset, allow_hyphen_values = true, default_value = "0")]
    offset: u64,
    /// Output plain hex
    #[arg(short, long, visible_aliases = ["ps", "postscript"])]
    plain: bool,
    /// Convert a hex dump back to binary
    #[arg(short, long)]
    revert: bool,
    /// Colorize the output
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto)]
    color: ColorChoice,
    /// Start at this infile offset ('+': relative, '-': from end); with -r, add it to the offsets
    #[arg(short, long, value_parser = parse_seek, allow_hyphen_values = true)]
    seek: Option<SeekOffset>,
    /// Use upper case hex letters
    #[arg(short, long)]
    uppercase: bool,
    #[arg(value_name = "INFILE", default_value = "-")]
    infile: Source,
    #[arg(value_name = "OUTFILE", default_value = "-")]
    outfile: PathBuf,
}

#[derive(Clone, Copy, ValueEnum)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy)]
struct SeekOffset {
    relative: bool,
    from_end: bool,
    amount: u64,
}

impl SeekOffset {
    fn signed(self) -> Result<i64> {
        let amount = i64::try_from(self.amount).context("seek offset too large")?;
        Ok(if self.from_end { -amount } else { amount })
    }
}

impl Tool for Hexd {
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

impl Hexd {
    fn format(&self) -> Format {
        if self.plain {
            Format::Plain
        } else if self.include {
            Format::Include { bits: self.bits }
        } else if self.bits {
            Format::Lines(Style::Bits)
        } else if self.little_endian {
            Format::Lines(Style::LittleEndian)
        } else {
            Format::Lines(Style::Hex)
        }
    }
}

fn run(args: Hexd) -> Result<()> {
    let format = args.format();
    let cols = match args.cols {
        Some(cols) if cols > 0 || matches!(format, Format::Plain) => cols,
        _ => format.default_cols(),
    };
    if matches!(format, Format::Lines(_)) && cols > MAX_COLS {
        bail!("invalid number of columns (max. {MAX_COLS})");
    }
    let group = match format {
        Format::Lines(style) => style.group(args.groupsize, cols)?,
        Format::Plain | Format::Include { .. } => 0,
    };

    let mut input = open_input(&args.infile)?;
    let to_stdout = args.outfile.as_os_str() == "-";
    let output = if to_stdout {
        File::from(
            io::stdout()
                .as_fd()
                .try_clone_to_owned()
                .context("opening stdout")?,
        )
    } else {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(!args.revert)
            .open(&args.outfile)
            .with_context(|| format!("opening {}", args.outfile.display()))?
    };
    let mut output = BufWriter::new(output);

    if args.revert {
        let kind = match format {
            Format::Plain => Revert::Plain,
            Format::Lines(Style::Bits) => Revert::Bits,
            Format::Lines(_) | Format::Include { .. } => Revert::Hex,
        };
        let base = args.seek.map_or(Ok(0), SeekOffset::signed)?;
        revert(input, &mut output, cols, kind, base).context("reverting")?;
    } else {
        let start = match args.seek {
            Some(seek) if !(seek.relative && !seek.from_end && seek.amount == 0) => {
                seek_input(&mut input, seek)?
            }
            _ => 0,
        };
        let input = input.take(args.len.unwrap_or(u64::MAX));
        let dumped = match format {
            Format::Plain => {
                let digits = if args.uppercase { UPPER } else { LOWER };
                plain(input, &mut output, cols, digits)
            }
            Format::Include { bits } => {
                let name = match &args.infile {
                    Source::File(path) => Some(path.as_os_str().as_bytes()),
                    Source::Stdin => None,
                };
                include(input, &mut output, cols, args.uppercase, bits, name)
            }
            Format::Lines(style) => Lines {
                style,
                cols,
                group,
                digits: if args.uppercase { UPPER } else { LOWER },
                decimal: args.decimal,
                base: start.wrapping_add(args.offset),
                color: use_color(args.color, to_stdout),
                autoskip: args.autoskip,
            }
            .dump(input, &mut output),
        };
        dumped.context("dumping")?;
    }
    output.flush()?;
    Ok(())
}

/// Writes `input` to `output` as an xxd-style hex dump.
pub fn dump(input: impl Read, output: impl Write) -> io::Result<()> {
    Lines {
        style: Style::Hex,
        cols: 16,
        group: 2,
        digits: LOWER,
        decimal: false,
        base: 0,
        color: false,
        autoskip: false,
    }
    .dump(input, output)
}

#[derive(Clone, Copy)]
enum Format {
    Lines(Style),
    Plain,
    Include { bits: bool },
}

impl Format {
    const fn default_cols(self) -> usize {
        match self {
            Format::Plain => 30,
            Format::Include { bits: false } => 12,
            Format::Include { bits: true } | Format::Lines(Style::Bits) => 6,
            Format::Lines(Style::Hex | Style::LittleEndian) => 16,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Hex,
    LittleEndian,
    Bits,
}

impl Style {
    fn group(self, requested: Option<usize>, cols: usize) -> Result<usize> {
        let group = requested.unwrap_or(match self {
            Style::Hex => 2,
            Style::LittleEndian => 4,
            Style::Bits => 1,
        });
        if group < 1 || group > cols {
            Ok(cols)
        } else if self == Style::LittleEndian && !group.is_power_of_two() {
            bail!("number of octets per group must be a power of 2 with -e")
        } else {
            Ok(group)
        }
    }
}

fn open_input(source: &Source) -> Result<File> {
    match source {
        Source::Stdin if io::stdin().is_terminal() => bail!("stdin is a terminal"),
        Source::Stdin => Ok(File::from(
            io::stdin()
                .as_fd()
                .try_clone_to_owned()
                .context("opening stdin")?,
        )),
        Source::File(path) => {
            File::open(path).with_context(|| format!("opening {}", path.display()))
        }
    }
}

/// Moves `input` to the `-s` position and returns the absolute offset reached.
fn seek_input(input: &mut File, seek: SeekOffset) -> Result<u64> {
    let target = match (seek.relative, seek.from_end) {
        (false, false) => SeekFrom::Start(seek.amount),
        (false, true) => SeekFrom::End(seek.signed()?),
        (true, _) => SeekFrom::Current(seek.signed()?),
    };
    match input.seek(target) {
        Ok(position) => Ok(position),
        Err(_) if seek.from_end => bail!("cannot seek"),
        Err(_) => {
            let skipped = io::copy(&mut Read::by_ref(input).take(seek.amount), &mut io::sink())
                .context("seeking")?;
            if skipped < seek.amount {
                bail!("cannot seek");
            }
            Ok(seek.amount)
        }
    }
}

fn use_color(when: ColorChoice, to_stdout: bool) -> bool {
    match when {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => {
            to_stdout
                && env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
                && env::var_os("TERM").is_some_and(|term| !term.is_empty() && term != "dumb")
                && io::stdout().is_terminal()
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
enum Color {
    Red = b'1',
    Green = b'2',
    Yellow = b'3',
    Blue = b'4',
    White = b'7',
}

impl Color {
    const fn of(byte: u8) -> Self {
        match byte {
            32..=126 => Color::Green,
            b'\t' | b'\n' | b'\r' => Color::Yellow,
            0 => Color::White,
            255 => Color::Blue,
            _ => Color::Red,
        }
    }
}

struct Lines {
    style: Style,
    cols: usize,
    group: usize,
    digits: &'static [u8; 16],
    decimal: bool,
    base: u64,
    color: bool,
    autoskip: bool,
}

impl Lines {
    fn dump(&self, input: impl Read, mut output: impl Write) -> io::Result<()> {
        let mut input = BufReader::new(input);
        let mut chunk = Vec::with_capacity(self.cols);
        let mut line = Vec::new();
        let mut autoskip = Autoskip::default();
        let mut offset: u64 = 0;
        loop {
            chunk.clear();
            input
                .by_ref()
                .take(self.cols as u64)
                .read_to_end(&mut chunk)?;
            if chunk.is_empty() {
                if self.autoskip {
                    autoskip.write(&mut output, &line, LineKind::Last)?;
                }
                return Ok(());
            }
            line.clear();
            self.render(offset.wrapping_add(self.base), &chunk, &mut line);
            offset += chunk.len() as u64;
            if chunk.len() < self.cols {
                return autoskip.write(&mut output, &line, LineKind::Shown);
            }
            let kind = if self.autoskip && chunk.iter().all(|&byte| byte == 0) {
                LineKind::Zero
            } else {
                LineKind::Shown
            };
            autoskip.write(&mut output, &line, kind)?;
        }
    }

    fn render(&self, address: u64, bytes: &[u8], out: &mut Vec<u8>) {
        let address = if self.decimal {
            format!("{address:08}:")
        } else {
            format!("{address:08x}:")
        };
        let start = address.len();
        let group = self.group;
        let group_width = match self.style {
            Style::Bits => 8 * group + 1,
            Style::Hex | Style::LittleEndian => 2 * group + 1,
        };
        let text = start
            + 3
            + match self.style {
                Style::LittleEndian => group_width * self.cols.div_ceil(group) - 1,
                Style::Hex | Style::Bits => (group_width * self.cols - 1) / group,
            };

        let mut cells = Cells::new(address.as_bytes());
        for (position, &byte) in bytes.iter().enumerate() {
            let slot = match self.style {
                Style::LittleEndian => position ^ (group - 1),
                Style::Hex | Style::Bits => position,
            };
            let column = start + 1 + group_width * slot / group;
            let color = self.color.then(|| Color::of(byte));
            match self.style {
                Style::Bits => {
                    for bit in 0..8 {
                        let digit = if byte & (0x80 >> bit) == 0 {
                            b'0'
                        } else {
                            b'1'
                        };
                        cells.put(column + bit, digit, color);
                    }
                }
                Style::Hex | Style::LittleEndian => {
                    cells.put(column, self.digits[usize::from(byte >> 4)], color);
                    cells.put(column + 1, self.digits[usize::from(byte & 0xf)], color);
                }
            }
            let shown = if (32..127).contains(&byte) {
                byte
            } else {
                b'.'
            };
            cells.put(text + position, shown, color);
        }
        cells.resize(text + bytes.len());
        cells.write_to(out);
    }
}

/// One output line, with an optional color per character.
struct Cells {
    text: Vec<u8>,
    colors: Vec<Option<Color>>,
}

impl Cells {
    fn new(prefix: &[u8]) -> Self {
        Self {
            text: prefix.to_vec(),
            colors: vec![None; prefix.len()],
        }
    }

    fn resize(&mut self, len: usize) {
        self.text.resize(len, b' ');
        self.colors.resize(len, None);
    }

    fn put(&mut self, index: usize, byte: u8, color: Option<Color>) {
        if index >= self.text.len() {
            self.resize(index + 1);
        }
        self.text[index] = byte;
        self.colors[index] = color;
    }

    fn write_to(&self, out: &mut Vec<u8>) {
        let mut current = None;
        for (&byte, &color) in self.text.iter().zip(&self.colors) {
            if color != current {
                if current.is_some() {
                    out.extend_from_slice(b"\x1b[0m");
                }
                if let Some(color) = color {
                    out.extend_from_slice(&[0x1b, b'[', b'1', b';', b'3', color as u8, b'm']);
                }
                current = color;
            }
            out.push(byte);
        }
        if current.is_some() {
            out.extend_from_slice(b"\x1b[0m");
        }
        out.push(b'\n');
    }
}

#[derive(Clone, Copy, PartialEq)]
enum LineKind {
    /// Always shown.
    Shown,
    /// All zeros; may be folded into '*'.
    Zero,
    /// Repeats the final line at end of input so trailing zero lines end with it.
    Last,
}

/// Folds three or more consecutive zero lines into '*', as `xxd -a` does.
#[derive(Default)]
struct Autoskip {
    zeros_seen: u8,
    second_zero: Vec<u8>,
}

impl Autoskip {
    fn write(&mut self, output: &mut impl Write, line: &[u8], kind: LineKind) -> io::Result<()> {
        if kind == LineKind::Zero {
            let prior = self.zeros_seen;
            if prior == 1 {
                self.second_zero.clear();
                self.second_zero.extend_from_slice(line);
            }
            self.zeros_seen = (prior + 1).min(4);
            if prior == 0 {
                output.write_all(line)?;
            }
            return Ok(());
        }
        if kind == LineKind::Last {
            self.zeros_seen = self.zeros_seen.saturating_sub(1);
        }
        match self.zeros_seen {
            2 => output.write_all(&self.second_zero)?,
            3.. => output.write_all(b"*\n")?,
            _ => {}
        }
        if kind == LineKind::Shown || self.zeros_seen > 0 {
            output.write_all(line)?;
        }
        self.zeros_seen = 0;
        Ok(())
    }
}

fn plain(
    input: impl Read,
    mut output: impl Write,
    cols: usize,
    digits: &[u8; 16],
) -> io::Result<()> {
    let mut column = 0;
    for byte in BufReader::new(input).bytes() {
        let byte = byte?;
        output.write_all(&[
            digits[usize::from(byte >> 4)],
            digits[usize::from(byte & 0xf)],
        ])?;
        column += 1;
        if column == cols {
            output.write_all(b"\n")?;
            column = 0;
        }
    }
    if cols == 0 || column > 0 {
        output.write_all(b"\n")?;
    }
    Ok(())
}

fn include(
    input: impl Read,
    mut output: impl Write,
    cols: usize,
    uppercase: bool,
    bits: bool,
    name: Option<&[u8]>,
) -> io::Result<()> {
    let name = name.map(variable_name);
    if let Some(name) = &name {
        output.write_all(b"unsigned char ")?;
        output.write_all(name)?;
        output.write_all(b"[] = {\n")?;
    }
    let mut count: u64 = 0;
    for byte in BufReader::new(input).bytes() {
        let byte = byte?;
        let separator = if count == 0 {
            "  "
        } else if count.is_multiple_of(cols as u64) {
            ",\n  "
        } else {
            ", "
        };
        if bits {
            write!(output, "{separator}0b{byte:08b}")?;
        } else if uppercase {
            write!(output, "{separator}0X{byte:02X}")?;
        } else {
            write!(output, "{separator}0x{byte:02x}")?;
        }
        count += 1;
    }
    if count > 0 {
        output.write_all(b"\n")?;
    }
    if let Some(name) = &name {
        output.write_all(b"};\nunsigned int ")?;
        output.write_all(name)?;
        writeln!(output, "_len = {count};")?;
    }
    Ok(())
}

/// Turns a file name into a C identifier the way xxd does.
fn variable_name(raw: &[u8]) -> Vec<u8> {
    let mut name = if raw.first().is_some_and(u8::is_ascii_digit) {
        b"__".to_vec()
    } else {
        Vec::new()
    };
    name.extend(raw.iter().map(|&byte| {
        if byte.is_ascii_alphanumeric() {
            byte
        } else {
            b'_'
        }
    }));
    name
}

#[derive(Clone, Copy, PartialEq)]
enum Revert {
    Hex,
    Plain,
    Bits,
}

/// Ports xxd's `huntype`: decodes up to `cols` octets per line, seeking (or
/// zero-filling) `output` to each line's offset plus `base`.
fn revert<W: Write + Seek>(
    input: impl Read,
    output: &mut W,
    cols: usize,
    kind: Revert,
    base: i64,
) -> Result<()> {
    let mut bytes = BufReader::new(input).bytes();
    let mut ignore_garbage = true;
    let (mut n1, mut n2, mut n3): (Option<u8>, Option<u8>, Option<u8>) = (None, Some(0), Some(0));
    let mut decoded = cols;
    let (mut bits, mut bit_count) = (0u8, 0u8);
    let (mut have, mut want) = (0i64, 0i64);

    while let Some(byte) = bytes.next().transpose()? {
        let mut current = Some(byte);
        if byte == b'\r' {
            continue;
        }
        if kind == Revert::Plain && matches!(byte, b' ' | b'\n' | b'\t') {
            continue;
        }
        if kind == Revert::Bits {
            n1 = hex_digit(byte);
            if n1.is_none() && ignore_garbage {
                continue;
            }
            if let Some(bit) = bin_digit(byte) {
                bits = (bits << 1) | bit;
                bit_count += 1;
            }
        } else {
            n3 = n2;
            n2 = n1;
            n1 = hex_digit(byte);
            if n1.is_none() && ignore_garbage {
                continue;
            }
        }
        ignore_garbage = false;

        if kind != Revert::Plain && decoded >= cols {
            match n1 {
                Some(digit) => want = (want << 4) | i64::from(digit),
                None => {
                    decoded = 0;
                    if kind == Revert::Bits {
                        bit_count = 0;
                    }
                }
            }
            continue;
        }

        let target = base.wrapping_add(want);
        if target != have {
            output.flush()?;
            if output
                .seek(SeekFrom::Current(target.wrapping_sub(have)))
                .is_ok()
            {
                have = target;
            }
            if target < have {
                bail!("cannot seek backwards");
            }
            while have < target {
                output.write_all(&[0])?;
                have += 1;
            }
        }

        if kind == Revert::Bits {
            if bit_count == 8 {
                output.write_all(&[bits])?;
                have += 1;
                want += 1;
                bits = 0;
                bit_count = 0;
                decoded += 1;
                if decoded >= cols {
                    current = skip_to_eol(&mut bytes, current)?;
                }
            }
        } else if let (Some(high), Some(low)) = (n2, n1) {
            output.write_all(&[(high << 4) | low])?;
            have += 1;
            want += 1;
            n1 = None;
            if kind == Revert::Hex {
                decoded += 1;
                if decoded >= cols {
                    current = skip_to_eol(&mut bytes, current)?;
                }
            }
        } else if n1.is_none() && n2.is_none() && n3.is_none() {
            current = skip_to_eol(&mut bytes, current)?;
        }

        if current == Some(b'\n') {
            if kind != Revert::Plain {
                want = 0;
            }
            decoded = cols;
            ignore_garbage = true;
        }
    }
    output.flush()?;
    Ok(())
}

fn skip_to_eol(
    bytes: &mut impl Iterator<Item = io::Result<u8>>,
    mut current: Option<u8>,
) -> io::Result<Option<u8>> {
    while current.is_some_and(|byte| byte != b'\n') {
        current = bytes.next().transpose()?;
    }
    Ok(current)
}

fn hex_digit(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(16).map(|digit| digit as u8)
}

fn bin_digit(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(2).map(|digit| digit as u8)
}

/// Parses a number the way C's `strtol` does with base 0: `0x` hex, leading `0` octal.
fn parse_number(value: &str) -> Result<u64, String> {
    let (digits, radix) = match value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        Some(hex) => (hex, 16),
        None if value.len() > 1 && value.starts_with('0') => (&value[1..], 8),
        None => (value, 10),
    };
    u64::from_str_radix(digits, radix).map_err(|error| error.to_string())
}

fn parse_size(value: &str) -> Result<usize, String> {
    usize::try_from(parse_number(value)?).map_err(|error| error.to_string())
}

fn parse_offset(value: &str) -> Result<u64, String> {
    let value = value.strip_prefix('+').unwrap_or(value);
    match value.strip_prefix('-') {
        Some(amount) => parse_number(amount).map(u64::wrapping_neg),
        None => parse_number(value),
    }
}

fn parse_seek(value: &str) -> Result<SeekOffset, String> {
    let (relative, value) = match value.strip_prefix('+') {
        Some(rest) => (true, rest),
        None => (false, value),
    };
    let (from_end, value) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value),
    };
    Ok(SeekOffset {
        relative,
        from_end,
        amount: parse_number(value)?,
    })
}
