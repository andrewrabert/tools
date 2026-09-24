use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use bat::PrettyPrinter;
use bat::line_range::{LineRange, LineRanges};

use crate::tools::archive::{Names, extract_member, list};
use crate::tools::hexd;
use crate::tools::mkv_preview;
use crate::tools::preview::config::Color;
use crate::tools::preview::kind::Kind;
use crate::tools::preview::mime::MimeType;

const TEXT_LINE_LIMIT: usize = 1024;
const BINARY_BYTE_LIMIT: u64 = 65536;
const DELIMITER_WIDTH: usize = 40;
const IDENTIFY_FORMAT: &str = r"File name       : %m\nResolution      : %wx%h\nColorspace      : %[colorspace]\nBit-depth       : %z\n";
const EXIV2_OMITTED_FIELDS: [&[u8]; 4] = [
    b"File name       : ",
    b"Image size      : ",
    b"File size       : ",
    b"MIME type       : ",
];

pub struct Failure {
    pub partial: Vec<u8>,
    pub error: anyhow::Error,
}

pub fn file(file: &Path, color: &Color) -> Result<Vec<u8>, Failure> {
    let mut out = Vec::new();
    let result = (|| -> Result<()> {
        let mut push = |result: Result<Vec<u8>, Failure>| match result {
            Ok(data) => {
                out.extend(data);
                Ok(())
            }
            Err(failure) => {
                out.extend(failure.partial);
                Err(failure.error)
            }
        };
        if file.is_dir() {
            bail!("{} is a directory", file.display());
        }
        let mime = MimeType::detect(file)?;
        match Kind::of(&mime, file) {
            Kind::Archive => push(list_archive(file)),
            Kind::CueSheet => {
                if let Ok(contents) = list(file) {
                    push(Ok(contents.lines(&Names::Raw)))?;
                }
                push(Ok(
                    format!("\n{}\n\n", "-".repeat(DELIMITER_WIDTH)).into_bytes()
                ))?;
                let mut source =
                    File::open(file).with_context(|| format!("opening {}", file.display()))?;
                let mut data = Vec::new();
                let copied = source.read_to_end(&mut data);
                push(Ok(data))?;
                copied?;
                Ok(())
            }
            Kind::JavaArchive => {
                // A temporary file avoids deadlocking on a pipe the extraction fills.
                let mut scratch = tempfile::tempfile().context("creating a temporary file")?;
                extract_member(file, "META-INF/MANIFEST.MF", scratch.try_clone()?.into())?;
                scratch.seek(SeekFrom::Start(0))?;
                let mut manifest = Vec::new();
                scratch
                    .read_to_end(&mut manifest)
                    .context("reading the manifest")?;
                push(Ok(manifest))
            }
            Kind::Text => push(highlight(file, color)),
            Kind::Matroska => push(Ok(mkv_preview::summary(file)?)),
            Kind::Media => {
                let output = Command::new("ffprobe")
                    .args(["-hide_banner", "--"])
                    .arg(file)
                    .stdin(Stdio::inherit())
                    .output()
                    .context("running ffprobe")?;
                push(Ok(output.stdout))?;
                push(Ok(output.stderr))?;
                if !output.status.success() {
                    return Err(anyhow!("ffprobe failed: {}", output.status));
                }
                Ok(())
            }
            Kind::Epub => push(run(Command::new("epubmeta").arg("--").arg(file))),
            Kind::Executable => {
                if let Ok(output) = exiftool(file)
                    .stdin(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .output()
                {
                    push(Ok(output.stdout))?;
                }
                push(list_archive(file))
            }
            Kind::Exif => push(run(&mut exiftool(file))),
            Kind::IccProfile => push(run(Command::new("iccdump").args(["-t", "desc"]).arg(file))),
            Kind::Jxl => {
                push(identify(file))?;
                push(Ok(exiv2(file)?))?;
                push(run(Command::new("jxlinfo").arg("--verbose").arg(file)))
            }
            Kind::Tiff => push(run(Command::new("tiffinfo").arg("--").arg(file))),
            Kind::BareImage => push(identify(file)),
            Kind::Image => {
                push(identify(file))?;
                push(Ok(exiv2(file)?))
            }
            Kind::Binary => {
                push(Ok(format!("binary file {}\n", file.display()).into_bytes()))?;
                push(hex_dump(file))
            }
        }
    })();
    match result {
        Ok(()) => Ok(out),
        Err(error) => Err(Failure {
            partial: out,
            error,
        }),
    }
}

fn run(command: &mut Command) -> Result<Vec<u8>, Failure> {
    let program = command.get_program().display().to_string();
    let output = match command
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            return Err(Failure {
                partial: Vec::new(),
                error: anyhow::Error::new(error).context(format!("running {program}")),
            });
        }
    };
    if !output.status.success() {
        return Err(Failure {
            partial: output.stdout,
            error: anyhow!("{program} failed: {}", output.status),
        });
    }
    Ok(output.stdout)
}

fn highlight(file: &Path, color: &Color) -> Result<Vec<u8>, Failure> {
    let mut out = String::new();
    let result = PrettyPrinter::new()
        .input_file(file)
        .colored_output(matches!(color, Color::Always))
        .theme("ansi")
        .line_ranges(LineRanges::from(vec![LineRange::new(1, TEXT_LINE_LIMIT)]))
        .print_with_writer(Some(&mut out));
    let error = match result {
        Ok(true) => return Ok(out.into_bytes()),
        Ok(false) => anyhow!("highlighting {} failed", file.display()),
        Err(error) => anyhow::Error::new(error).context(format!("highlighting {}", file.display())),
    };
    Err(Failure {
        partial: out.into_bytes(),
        error,
    })
}

fn list_archive(file: &Path) -> Result<Vec<u8>, Failure> {
    match list(file) {
        Ok(contents) => Ok(contents.lines(&Names::Raw)),
        Err(error) => Err(Failure {
            partial: Vec::new(),
            error,
        }),
    }
}

fn hex_dump(file: &Path) -> Result<Vec<u8>, Failure> {
    let mut out = Vec::new();
    File::open(file)
        .and_then(|source| hexd::dump(source.take(BINARY_BYTE_LIMIT), &mut out))
        .with_context(|| format!("reading {}", file.display()))
        .map_err(|error| Failure {
            partial: Vec::new(),
            error,
        })?;
    Ok(out)
}

fn exiftool(file: &Path) -> Command {
    let mut command = Command::new("exiftool");
    command.arg("--").arg(file);
    command
}

fn identify(file: &Path) -> Result<Vec<u8>, Failure> {
    run(Command::new("magick")
        .args(["identify", "-quiet", "-format", IDENTIFY_FORMAT, "--"])
        .arg(file))
}

fn exiv2(file: &Path) -> Result<Vec<u8>> {
    let Ok(output) = Command::new("exiv2")
        .arg(file)
        .stderr(Stdio::null())
        .output()
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for line in output.stdout.split_inclusive(|byte| *byte == b'\n') {
        if !EXIV2_OMITTED_FIELDS
            .iter()
            .any(|field| line.starts_with(field))
        {
            out.extend_from_slice(line);
        }
    }
    Ok(out)
}
