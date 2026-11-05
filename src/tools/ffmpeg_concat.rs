use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{self, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use jiff::Zoned;
use tempfile::NamedTempFile;

use crate::tools::Tool;
use crate::{atomic_write, ffmpeg, walk};

const EXTENSION: &str = "mkv";
const TEMP_PREFIX: &str = ".ffmpeg-concat_";

#[derive(ClapArgs)]
#[command(about = "Join the videos under a directory, in path order, into one Matroska file")]
pub struct FfmpegConcat {
    /// Remove the source files once joined
    #[arg(long)]
    rm: bool,
    /// The .mkv file to write, or the directory to write it in [default: the current directory]
    #[arg(short = 'o', value_name = "OUTPUT")]
    output: Option<PathBuf>,
    #[arg(value_name = "PATH")]
    path: PathBuf,
}

impl Tool for FfmpegConcat {
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

fn run(args: FfmpegConcat) -> Result<()> {
    let dest = destination(args.output.as_deref())?;
    if dest.exists() {
        bail!("{} already exists", dest.display());
    }
    let sources = files(&args.path)?;
    if sources.is_empty() {
        bail!("no files under {}", args.path.display());
    }

    let mut list = NamedTempFile::new().context("creating the concat list")?;
    list.write_all(&concat_list(&sources)?)
        .and_then(|()| list.flush())
        .context("writing the concat list")?;
    let temp = atomic_write::sibling(TEMP_PREFIX, &dest)?;
    ffmpeg::run(
        ffmpeg::command()
            .args(["-f", "concat", "-safe", "0", "-i"])
            .arg(list.path())
            .args(["-c", "copy", "-f", "matroska", "-y"])
            .arg(&temp),
    )?;
    temp.persist_noclobber(&dest)
        .with_context(|| format!("writing {}", dest.display()))?;

    if args.rm {
        for source in &sources {
            fs::remove_file(source).with_context(|| format!("removing {}", source.display()))?;
        }
    }
    Ok(())
}

/// The file `-o` names, or a timestamped file in the directory it names.
fn destination(output: Option<&Path>) -> Result<PathBuf> {
    let dir = match output {
        None => Path::new("."),
        Some(output) if output.is_dir() => output,
        Some(output) => {
            let parent = output
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            if output.extension() != Some(OsStr::new(EXTENSION)) || !parent.is_dir() {
                bail!(
                    "{} is neither a directory nor a .{EXTENSION} file in one",
                    output.display()
                );
            }
            return Ok(output.to_path_buf());
        }
    };
    let timestamp = Zoned::now().strftime("%Y%m%dT%H%M%S");
    Ok(dir.join(format!("ffmpeg_concat_{timestamp}.{EXTENSION}")))
}

/// The regular files under `root`, absolute and sorted.
fn files(root: &Path) -> Result<Vec<PathBuf>> {
    let root = path::absolute(root).with_context(|| format!("resolving {}", root.display()))?;
    let mut files = Vec::new();
    for entry in walk::entries(&root) {
        let entry = entry?;
        if entry.file_type().is_file() {
            files.push(entry.into_path());
        }
    }
    files.sort();
    Ok(files)
}

/// The input of ffmpeg's concat demuxer, listing `paths` in order.
fn concat_list(paths: &[PathBuf]) -> Result<Vec<u8>> {
    let mut list = Vec::new();
    for path in paths {
        let bytes = path.as_os_str().as_bytes();
        if bytes.contains(&b'\n') {
            bail!("{} contains a newline", path.display());
        }
        list.extend_from_slice(b"file '");
        for &byte in bytes {
            if byte == b'\'' {
                list.extend_from_slice(br"'\''");
            } else {
                list.push(byte);
            }
        }
        list.extend_from_slice(b"'\n");
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concat_list_quotes_each_path() -> Result<()> {
        let paths = [PathBuf::from("/a/1.mkv"), PathBuf::from("/a/it's.mkv")];
        assert_eq!(
            concat_list(&paths)?,
            b"file '/a/1.mkv'\nfile '/a/it'\\''s.mkv'\n"
        );
        Ok(())
    }

    #[test]
    fn concat_list_rejects_newlines() {
        assert!(concat_list(&[PathBuf::from("/a/b\nc")]).is_err());
    }

    #[test]
    fn destination_rejects_other_extensions() {
        assert!(destination(Some(Path::new("out.mp4"))).is_err());
    }

    #[test]
    fn destination_accepts_mkv_in_current_directory() -> Result<()> {
        assert_eq!(
            destination(Some(Path::new("out.mkv")))?,
            PathBuf::from("out.mkv")
        );
        Ok(())
    }
}
