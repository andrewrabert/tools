use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::tools::archive::create::config::Speed;
use crate::tools::archive::create::format::Format;
use crate::tools::archive::create::members::Members;
use crate::tools::archive::extract::seven_zip_program;
use crate::tools::archive::password::Password;
use crate::tools::archive::process;
use crate::tools::archive::temp;

const TEMP_PREFIX: &str = ".archive_";
const ISO_VOLUME_ID_LENGTH: usize = 32;

enum TarFilter {
    None,
    Program(&'static str),
}

enum TarHoles {
    Stored,
    Sparse,
}

pub fn write(
    format: Format,
    source: &Path,
    dest: &Path,
    speed: Speed,
    password: Option<&Password>,
) -> Result<()> {
    match format {
        Format::SevenZip => seven_zip(source, dest, speed, password),
        Format::Cbz | Format::Zip => zip(&Members::of(source)?, dest, speed),
        Format::Epub => epub(source, dest, speed),
        Format::Iso => iso(source, dest),
        Format::Gz => gzip(file(source)?, dest, speed),
        Format::Nsz => nsz(file(source)?, dest, speed),
        Format::Rvz => rvz(file(source)?, dest, speed),
        Format::Tar => tar(source, dest, TarFilter::None),
        Format::TarGz => tar(
            source,
            dest,
            TarFilter::Program(match speed {
                Speed::Fast => "gzip --fast",
                Speed::Best => "gzip --best",
            }),
        ),
        Format::TarXz => tar(
            source,
            dest,
            TarFilter::Program(match speed {
                Speed::Fast => "xz --threads=0 -0",
                Speed::Best => "xz --threads=0 -9 --extreme",
            }),
        ),
        Format::TarZst => tar_zstd(source, dest, speed, TarHoles::Stored),
        Format::SparseTarZst => tar_zstd(source, dest, speed, TarHoles::Sparse),
        Format::Xz => xz(file(source)?, dest, speed),
        Format::Zst => zstd(file(source)?, dest, speed),
    }
}

fn file(source: &Path) -> Result<&Path> {
    if source.is_dir() {
        bail!("{} is a directory", source.display());
    }
    Ok(source)
}

fn zip(members: &Members, dest: &Path, speed: Speed) -> Result<()> {
    // zip updates an existing file, and the empty temporary file is not a zip.
    fs::remove_file(dest).with_context(|| format!("removing {}", dest.display()))?;
    process::feed(
        Command::new("zip")
            .arg(match speed {
                Speed::Fast => "-0",
                Speed::Best => "-9",
            })
            // EpubCheck rejects the extra file attributes that -X leaves out.
            .args(["-nw", "-X", "-@", "-q"])
            .arg(dest)
            .current_dir(&members.cwd),
        &members.joined(b'\n'),
    )?;
    if let Speed::Best = speed {
        process::run(
            Command::new("advzip")
                .args(["--recompress", "--shrink-insane", "--quiet"])
                .arg(dest),
        )?;
    }
    Ok(())
}

fn epub(source: &Path, dest: &Path, speed: Speed) -> Result<()> {
    if !source.is_dir() {
        bail!("epub requires source directory");
    }
    let mut members = Members::of(source)?;
    members
        .relative
        .sort_by(|a, b| (epub_rank(a), a).cmp(&(epub_rank(b), b)));
    zip(&members, dest, speed)
}

fn epub_rank(path: &Path) -> u8 {
    if path == Path::new("mimetype") {
        0
    } else if path.starts_with("META-INF") {
        1
    } else if path.starts_with("OEBPS") {
        2
    } else {
        3
    }
}

fn seven_zip(source: &Path, dest: &Path, speed: Speed, password: Option<&Password>) -> Result<()> {
    // 7z refuses to add to the empty temporary file.
    fs::remove_file(dest).with_context(|| format!("removing {}", dest.display()))?;
    let mut command = Command::new(seven_zip_program()?);
    command.arg("a").arg(match speed {
        Speed::Fast => "-mx=0",
        Speed::Best => "-mx=9",
    });
    if let Some(password) = password {
        command.arg("-mhe").arg(format!("-p{}", password.as_str()));
    }
    command.arg("--").arg(dest);
    if source.is_dir() {
        // Archiving "." keeps the contents from nesting under a single folder.
        command.arg(".").current_dir(source);
    } else {
        let parent = source
            .parent()
            .with_context(|| format!("{} has no parent", source.display()))?;
        command.arg(source).current_dir(parent);
    }
    process::run(command.stdout(Stdio::null()))
}

fn gzip(source: &Path, dest: &Path, speed: Speed) -> Result<()> {
    let output = File::create(dest).with_context(|| format!("opening {}", dest.display()))?;
    process::run(
        Command::new("gzip")
            .arg("-c")
            .arg(match speed {
                Speed::Fast => "-1",
                Speed::Best => "-9",
            })
            .arg(source)
            .stdout(output),
    )
}

fn xz(source: &Path, dest: &Path, speed: Speed) -> Result<()> {
    let output = File::create(dest).with_context(|| format!("opening {}", dest.display()))?;
    let mut command = Command::new("xz");
    command.args(["-c", "--threads=0"]);
    match speed {
        Speed::Fast => command.arg("-0"),
        Speed::Best => command.args(["-9", "--extreme"]),
    };
    process::run(command.arg(source).stdout(output))
}

fn zstd_level(speed: Speed) -> &'static str {
    match speed {
        Speed::Fast => "-1",
        Speed::Best => "-22",
    }
}

fn zstd(source: &Path, dest: &Path, speed: Speed) -> Result<()> {
    process::run(
        Command::new("zstd")
            .args(["--quiet", "-f", "-T0", "--ultra", zstd_level(speed), "-o"])
            .arg(dest)
            .arg("--")
            .arg(source),
    )
}

fn iso(source: &Path, dest: &Path) -> Result<()> {
    let name = source
        .file_name()
        .with_context(|| format!("{} has no file name", source.display()))?;
    let volume_id: String = name
        .to_string_lossy()
        .chars()
        .take(ISO_VOLUME_ID_LENGTH)
        .collect();
    process::run(
        Command::new("mkisofs")
            .args(["-J", "-volid", volume_id.as_str(), "-o"])
            .arg(dest)
            .arg("--")
            .arg(source),
    )
}

fn scratch_level(speed: Speed) -> &'static str {
    match speed {
        Speed::Fast => "1",
        Speed::Best => "22",
    }
}

fn nsz(source: &Path, dest: &Path, speed: Speed) -> Result<()> {
    let scratch = scratch_dir(dest)?;
    // nsz misbehaves with a closed stdout, so its output is read and dropped.
    process::capture(
        Command::new("nsz")
            .args(["-C", "--long"])
            .arg(format!("--level={}", scratch_level(speed)))
            .arg(process::prefixed("--output=", scratch.path().as_os_str()))
            .arg(source),
    )?;
    let entry = fs::read_dir(scratch.path())
        .with_context(|| format!("listing {}", scratch.path().display()))?
        .next()
        .context("nsz produced no file")?
        .with_context(|| format!("listing {}", scratch.path().display()))?;
    fs::rename(entry.path(), dest).with_context(|| format!("moving to {}", dest.display()))
}

fn rvz(source: &Path, dest: &Path, speed: Speed) -> Result<()> {
    // Concurrent dolphin-tool processes cannot share a user directory.
    let scratch = scratch_dir(dest)?;
    process::run(
        Command::new("dolphin-tool")
            .arg("convert")
            .arg(process::prefixed("--user=", scratch.path().as_os_str()))
            .arg(format!("--compression_level={}", scratch_level(speed)))
            .args(["--format=rvz", "--block_size=131072", "--compression=zstd"])
            .arg(process::prefixed("--input=", source.as_os_str()))
            .arg(process::prefixed("--output=", dest.as_os_str())),
    )
}

fn scratch_dir(dest: &Path) -> Result<tempfile::TempDir> {
    let parent = dest
        .parent()
        .with_context(|| format!("{} has no parent", dest.display()))?;
    let name = dest
        .file_name()
        .with_context(|| format!("{} has no file name", dest.display()))?;
    temp::dir(parent, name, TEMP_PREFIX)
}

fn tar_listing(members: &Members, archive: &Path, holes: &TarHoles) -> Command {
    let mut command = Command::new("tar");
    command
        .arg("cf")
        .arg(archive)
        .arg("-C")
        .arg(&members.cwd)
        .arg("--no-recursion");
    if let TarHoles::Sparse = holes {
        command.arg("--sparse");
    }
    command
}

fn tar(source: &Path, dest: &Path, filter: TarFilter) -> Result<()> {
    let members = Members::of(source)?;
    let mut command = tar_listing(&members, dest, &TarHoles::Stored);
    if let TarFilter::Program(program) = filter {
        command.args(["-I", program]);
    }
    process::feed(
        command.args(["--verbatim-files-from", "--null", "-T", "-"]),
        &members.joined(b'\0'),
    )
}

fn tar_zstd(source: &Path, dest: &Path, speed: Speed, holes: TarHoles) -> Result<()> {
    let members = Members::of(source)?;
    let mut tar = tar_listing(&members, Path::new("-"), &holes)
        .args(["--verbatim-files-from", "--null", "-T", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("running tar")?;
    let archive = tar.stdout.take().context("tar has no stdout")?;
    let mut zstd = Command::new("zstd")
        .args(["--quiet", "-f", "-T0", "--ultra", zstd_level(speed), "-o"])
        .arg(dest)
        .stdin(Stdio::from(archive))
        .spawn()
        .context("running zstd")?;

    let mut listing = tar.stdin.take().context("tar has no stdin")?;
    let written = listing.write_all(&members.joined(b'\0'));
    drop(listing);

    let tar_status = tar.wait().context("waiting for tar")?;
    let zstd_status = zstd.wait().context("waiting for zstd")?;
    written.context("writing to tar")?;
    if !tar_status.success() {
        bail!("tar failed: {tar_status}");
    }
    if !zstd_status.success() {
        bail!("zstd failed: {zstd_status}");
    }
    Ok(())
}
