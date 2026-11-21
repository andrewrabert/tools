//! ROM bytes read front to back from a ROM file or a single-file 7z archive.
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result, anyhow, bail};

use crate::tools::archive::{extract_member, is_seven_zip, list};

const SIGNATURE_LENGTH: u64 = 16;

pub struct RomReader {
    path: PathBuf,
    name: PathBuf,
    file: File,
    extraction: Option<JoinHandle<Result<()>>>,
    kept: Vec<u8>,
    keeping: bool,
    consumed: u64,
    position: u64,
}

impl RomReader {
    pub fn open(path: &Path) -> Result<RomReader> {
        let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut start = Vec::new();
        file.by_ref()
            .take(SIGNATURE_LENGTH)
            .read_to_end(&mut start)
            .with_context(|| format!("reading {}", path.display()))?;
        if !is_seven_zip(&start) {
            file.rewind()
                .with_context(|| format!("reading {}", path.display()))?;
            return Ok(RomReader::new(path, path.to_path_buf(), file, None));
        }
        let mut files = list(path)?.into_iter();
        let (Some((member, _)), None) = (files.next(), files.next()) else {
            bail!("{} must hold exactly one file", path.display());
        };
        let member = String::from_utf8(member)
            .with_context(|| format!("{}: the file name is not valid UTF-8", path.display()))?;
        let name = path.with_file_name(
            Path::new(&member)
                .file_name()
                .with_context(|| format!("{}: {member} has no file name", path.display()))?,
        );
        let (reader, writer) = io::pipe().context("creating a pipe")?;
        let archive = path.to_path_buf();
        let extraction = thread::spawn(move || {
            extract_member(&archive, &member, writer.into())
                .with_context(|| format!("extracting {member} from {}", archive.display()))
        });
        Ok(RomReader::new(
            path,
            name,
            File::from(OwnedFd::from(reader)),
            Some(extraction),
        ))
    }

    fn new(
        path: &Path,
        name: PathBuf,
        file: File,
        extraction: Option<JoinHandle<Result<()>>>,
    ) -> RomReader {
        RomReader {
            path: path.to_path_buf(),
            name,
            file,
            extraction,
            kept: Vec::new(),
            keeping: false,
            consumed: 0,
            position: 0,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> &Path {
        &self.name
    }

    pub fn keep(&mut self) {
        self.keeping = true;
    }

    pub fn stop_keeping(&mut self) {
        self.keeping = false;
    }

    pub fn finish(self) -> Result<()> {
        let RomReader {
            mut file,
            extraction,
            ..
        } = self;
        let Some(extraction) = extraction else {
            return Ok(());
        };
        let drained = io::copy(&mut file, &mut io::sink());
        drop(file);
        extraction
            .join()
            .map_err(|_| anyhow!("7z thread panicked"))??;
        drained.context("reading from 7z")?;
        Ok(())
    }
}

impl Read for RomReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if let Some(kept) = usize::try_from(self.position)
            .ok()
            .and_then(|at| self.kept.get(at..))
            .filter(|kept| !kept.is_empty())
        {
            let len = out.len().min(kept.len());
            out[..len].copy_from_slice(&kept[..len]);
            self.position += len as u64;
            if !self.keeping && self.position >= self.kept.len() as u64 {
                self.kept = Vec::new();
            }
            return Ok(len);
        }
        if self.position < self.consumed {
            return Err(io::Error::other(format!(
                "cannot read offset {:#x} again from a stream",
                self.position
            )));
        }
        let skip = self.position - self.consumed;
        if skip > 0 {
            let skipped = if self.keeping {
                self.file.by_ref().take(skip).read_to_end(&mut self.kept)? as u64
            } else {
                io::copy(&mut self.file.by_ref().take(skip), &mut io::sink())?
            };
            self.consumed += skipped;
            if skipped < skip {
                return Ok(0);
            }
        }
        let len = self.file.read(out)?;
        if self.keeping {
            self.kept.extend_from_slice(&out[..len]);
        }
        self.consumed += len as u64;
        self.position += len as u64;
        Ok(len)
    }
}

impl Seek for RomReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let SeekFrom::Start(offset) = to else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "a stream seeks only from its start",
            ));
        };
        self.position = offset;
        Ok(offset)
    }
}
