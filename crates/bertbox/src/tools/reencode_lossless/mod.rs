mod cli;
mod codec;

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::flac::FlacFile;
use lofty::tag::TagExt;

use crate::pool;
use crate::tools::Tool;
use crate::tools::archive::create::report::SizeReport;
use crate::tools::archive::temp;
pub use crate::tools::reencode_lossless::cli::ReencodeLossless;
use crate::tools::reencode_lossless::codec::{Decoder, Source, Target};
use crate::walk;

const TEMP_PREFIX: &str = ".reencode-";

impl Tool for ReencodeLossless {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: ReencodeLossless) -> Result<ExitCode> {
    let jobs = pool::jobs(args.num_procs)?;
    let mut sources = Vec::new();
    for path in &args.paths {
        if path.is_dir() {
            all_files(path, &mut sources)?;
        } else {
            sources.push(path.clone());
        }
    }
    sources.sort();
    sources.dedup();
    let sources: Vec<(PathBuf, Source)> = sources
        .into_iter()
        .filter_map(|path| Source::of(&path).map(|source| (path, source)))
        .collect();

    let mut report = SizeReport::new(sources.len());
    let mut errors = Vec::new();
    pool::run(
        sources,
        jobs,
        |(path, source)| reencode(path, *source, args.format, args.flac_decode_through_errors),
        |(path, _), result| {
            match result {
                Ok(Reencoded { before, after }) => report.print(before, after, &path),
                Err(error) => errors.push(format!("{}: {error:?}", path.display())),
            }
            Ok(())
        },
    )?;
    report.print_total();
    errors.sort();
    for error in &errors {
        eprintln!("Error: {error}");
    }
    Ok(if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn all_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in walk::entries(root) {
        let entry = entry?;
        if entry.file_type().is_file() {
            files.push(entry.into_path());
        }
    }
    Ok(())
}

struct Reencoded {
    before: u64,
    after: u64,
}

fn reencode(
    path: &Path,
    source: Source,
    target: Target,
    flac_decode_through_errors: bool,
) -> Result<Reencoded> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent", path.display()))?;
    let name = path
        .file_name()
        .with_context(|| format!("{} has no name", path.display()))?;
    let before = fs::metadata(path)
        .with_context(|| format!("reading the size of {}", path.display()))?
        .len();

    let tmp = temp::file(parent, name, TEMP_PREFIX)?;
    let decoder = source.decoder(path, flac_decode_through_errors)?;
    let (mut wav, mut decoding) = start_decoder(decoder)?;
    match target.encoder(&tmp) {
        Some(mut encoder) => {
            let status = encoder
                .stdin(Stdio::from(wav))
                .status()
                .context("running the encoder")?;
            if !status.success() {
                bail!("encoding failed: {status}");
            }
        }
        None => {
            let mut out =
                File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
            io::copy(&mut wav, &mut out).context("copying the audio")?;
        }
    }
    if let Some(child) = &mut decoding {
        let status = child.wait().context("waiting for the decoder")?;
        if !status.success() {
            bail!("decoding failed: {status}");
        }
    }

    // Tags are best effort, as not every source carries any.
    let _ = copy_tags(path, source, &tmp, target);
    fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
    let dest = path.with_extension(target.extension());
    tmp.persist(&dest)
        .with_context(|| format!("moving into place {}", dest.display()))?;
    let after = fs::metadata(&dest)
        .with_context(|| format!("reading the size of {}", dest.display()))?
        .len();
    Ok(Reencoded { before, after })
}

/// The raw WAV stream, plus the process producing it when there is one.
fn start_decoder(decoder: Decoder) -> Result<(Wav, Option<Child>)> {
    Ok(match decoder {
        Decoder::File(file) => (Wav::File(file), None),
        Decoder::Process(mut command) => {
            let mut child = command
                .stdout(Stdio::piped())
                .spawn()
                .context("running the decoder")?;
            let stdout = child.stdout.take().context("the decoder has no stdout")?;
            (Wav::Pipe(stdout), Some(child))
        }
    })
}

enum Wav {
    File(File),
    Pipe(ChildStdout),
}

impl Read for Wav {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Wav::File(file) => file.read(buf),
            Wav::Pipe(pipe) => pipe.read(buf),
        }
    }
}

impl From<Wav> for Stdio {
    fn from(wav: Wav) -> Self {
        match wav {
            Wav::File(file) => Stdio::from(file),
            Wav::Pipe(pipe) => Stdio::from(pipe),
        }
    }
}

fn copy_tags(from: &Path, source: Source, to: &Path, target: Target) -> Result<()> {
    if let (Source::Flac, Target::Flac) = (source, target) {
        let mut file = File::open(from)?;
        let flac = FlacFile::read_from(&mut file, ParseOptions::default())?;
        if let Some(comments) = flac.vorbis_comments() {
            comments.save_to_path(to, WriteOptions::default())?;
        }
        return Ok(());
    }
    let tagged = lofty::read_from_path(from)?;
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(());
    };
    let mut tag = tag.clone();
    tag.re_map(target.tag_type());
    tag.save_to_path(to, WriteOptions::default())?;
    Ok(())
}
