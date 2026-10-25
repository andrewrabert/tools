use std::collections::BTreeMap;
use std::fs::{self, Permissions};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use nix::unistd::Uid;
use tempfile::TempDir;
use winnow::Parser;
use winnow::combinator::terminated;
use winnow::error::{EmptyError, ParseError};
use winnow::token::{rest, take_until, take_while};

use crate::tools::archive::extract::backend::Request;
use crate::tools::archive::extract::listing::Contents;
use crate::tools::archive::process;
use crate::tools::archive::tree;

const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
const PARTITION_TABLE: &str = "org.freedesktop.UDisks2.PartitionTable";
const BLOCK_DEVICE_OBJECTS: &str = "/org/freedesktop/UDisks2/block_devices/";
const SECTION_INDENT: usize = 2;
const PROPERTY_INDENT: usize = 4;
const DIRECTORY_MODE: u32 = 0o755;
const FILE_MODE: u32 = 0o644;
const UNEXPECTED_OUTPUT: &str = "unexpected udisksctl info output";

type Info = BTreeMap<String, BTreeMap<String, Vec<String>>>;

fn info(device: &str) -> Result<Info> {
    let output = process::capture(
        Command::new("udisksctl")
            .args(["info", "--block-device"])
            .arg(device),
    )?;
    let output = String::from_utf8(output).context("udisksctl info is not valid UTF-8")?;
    parse_info(&output)
}

enum Line<'i> {
    Ignored,
    Section(&'i str),
    Property { name: &'i str, value: &'i str },
    Continuation(&'i str),
}

fn parse_info(output: &str) -> Result<Info> {
    let mut info = Info::new();
    let mut section = None;
    let mut property = None;
    for text in output.lines() {
        let value = match line
            .parse(text)
            .map_err(|_: ParseError<&str, EmptyError>| anyhow!(UNEXPECTED_OUTPUT))?
        {
            Line::Ignored => continue,
            Line::Section(name) => {
                info.insert(name.to_owned(), BTreeMap::new());
                section = Some(name);
                property = None;
                continue;
            }
            Line::Property { name, value } => {
                property = Some(name);
                value
            }
            Line::Continuation(value) => value,
        };
        let properties = section
            .and_then(|section| info.get_mut(section))
            .context(UNEXPECTED_OUTPUT)?;
        let name = property.context(UNEXPECTED_OUTPUT)?;
        if !value.is_empty() {
            properties
                .entry(name.to_owned())
                .or_default()
                .push(value.to_owned());
        }
    }
    Ok(info)
}

fn line<'i>(input: &mut &'i str) -> winnow::Result<Line<'i>, EmptyError> {
    let indent = whitespace.parse_next(input)?.len();
    if input.is_empty() || indent < SECTION_INDENT {
        rest.map(|_| Line::Ignored).parse_next(input)
    } else if indent < PROPERTY_INDENT {
        section.map(Line::Section).parse_next(input)
    } else if indent == PROPERTY_INDENT {
        property.parse_next(input)
    } else {
        continuation.map(Line::Continuation).parse_next(input)
    }
}

fn whitespace<'i>(input: &mut &'i str) -> winnow::Result<&'i str, EmptyError> {
    take_while(0.., char::is_whitespace).parse_next(input)
}

fn section<'i>(input: &mut &'i str) -> winnow::Result<&'i str, EmptyError> {
    rest.map(|header: &'i str| header.trim_end().trim_end_matches(':'))
        .parse_next(input)
}

fn property<'i>(input: &mut &'i str) -> winnow::Result<Line<'i>, EmptyError> {
    let name = terminated(take_until(0.., ':'), ':').parse_next(input)?;
    let value = continuation.parse_next(input)?;
    Ok(Line::Property { name, value })
}

fn continuation<'i>(input: &mut &'i str) -> winnow::Result<&'i str, EmptyError> {
    rest.map(str::trim).parse_next(input)
}

struct LoopDevice {
    device: String,
}

impl LoopDevice {
    fn attach(image: &Path) -> Result<Self> {
        let output = process::capture(
            Command::new("udisksctl")
                .args([
                    "loop-setup",
                    "--no-user-interaction",
                    "--read-only",
                    "--file",
                ])
                .arg(image),
        )?;
        let output = String::from_utf8_lossy(&output);
        let output = output.trim();
        let device = output
            .strip_prefix(&format!("Mapped file {} as ", image.display()))
            .and_then(|device| device.strip_suffix('.'))
            .with_context(|| format!("udisksctl error: {output}"))?;
        Ok(Self {
            device: device.to_owned(),
        })
    }
}

impl Drop for LoopDevice {
    fn drop(&mut self) {
        let deleted = process::run(
            Command::new("udisksctl")
                .args(["loop-delete", "--no-user-interaction", "--block-device"])
                .arg(&self.device)
                .stdout(Stdio::null()),
        );
        if let Err(error) = deleted {
            eprintln!("Error: {error:?}");
        }
    }
}

enum Location {
    Udisks { device: String, path: PathBuf },
    Root { directory: TempDir },
}

// The loop device is a field so that it outlives the unmount in `Drop`.
struct Mount {
    location: Location,
    _loop_device: LoopDevice,
}

impl Mount {
    fn of(image: &Path) -> Result<Self> {
        let loop_device = LoopDevice::attach(image)?;
        let device = filesystem_device(&loop_device.device)?;
        Ok(Self {
            location: Location::of(device)?,
            _loop_device: loop_device,
        })
    }

    fn path(&self) -> &Path {
        match &self.location {
            Location::Udisks { path, .. } => path,
            Location::Root { directory } => directory.path(),
        }
    }
}

impl Location {
    fn of(device: String) -> Result<Self> {
        let mounted = Command::new("udisksctl")
            .args([
                "mount",
                "--no-user-interaction",
                "--options",
                "ro",
                "--block-device",
            ])
            .arg(&device)
            .stderr(Stdio::null())
            .output()
            .context("running udisksctl")?;
        if mounted.status.success() {
            let output = String::from_utf8_lossy(&mounted.stdout);
            let output = output.trim();
            let path = output
                .strip_prefix(&format!("Mounted {device} at "))
                .with_context(|| format!("udisksctl error: {output}"))?;
            return Ok(Location::Udisks {
                path: PathBuf::from(path),
                device,
            });
        }

        let mount_point = info(&device)?
            .get(FILESYSTEM)
            .and_then(|filesystem| filesystem.get("MountPoints"))
            .and_then(|mount_points| mount_points.first().cloned());
        if let Some(path) = mount_point {
            return Ok(Location::Udisks {
                path: PathBuf::from(path),
                device,
            });
        }

        // udisksctl does not recognize some floppy images as mountable.
        if !Uid::current().is_root() {
            bail!("must be extracted as root");
        }
        let directory = tempfile::tempdir().context("creating a temporary directory")?;
        process::run(
            Command::new("mount")
                .args(["-o", "loop,ro"])
                .arg(&device)
                .arg(directory.path()),
        )?;
        Ok(Location::Root { directory })
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        let unmounted = match &self.location {
            Location::Udisks { device, .. } => process::run(
                Command::new("udisksctl")
                    .args(["unmount", "--no-user-interaction", "--block-device"])
                    .arg(device)
                    .stdout(Stdio::null()),
            ),
            Location::Root { directory } => {
                process::run(Command::new("umount").arg(directory.path()))
            }
        };
        if let Err(error) = unmounted {
            eprintln!("Error: {error:?}");
        }
    }
}

fn filesystem_device(loop_device: &str) -> Result<String> {
    let info = info(loop_device)?;
    if info.contains_key(FILESYSTEM) {
        bail!("{loop_device} holds a filesystem rather than a partition table");
    }
    let partitions = info
        .get(PARTITION_TABLE)
        .and_then(|table| table.get("Partitions"))
        .with_context(|| format!("{loop_device} has no partition table"))?;
    let mut filesystems = Vec::new();
    for partition in partitions {
        let name = partition
            .strip_prefix(BLOCK_DEVICE_OBJECTS)
            .with_context(|| format!("unexpected partition {partition}"))?;
        let device = format!("/dev/{name}");
        if self::info(&device)?.contains_key(FILESYSTEM) {
            filesystems.push(device);
        }
    }
    match filesystems.as_slice() {
        [device] => Ok(device.clone()),
        _ => bail!("{loop_device} does not hold exactly one filesystem"),
    }
}

pub fn contents(image: &Path) -> Result<Contents> {
    let mount = Mount::of(image)?;
    let mut names = Vec::new();
    for path in tree::paths(mount.path())? {
        if path != mount.path() {
            names.push(
                path.strip_prefix(mount.path())?
                    .as_os_str()
                    .as_bytes()
                    .to_vec(),
            );
        }
    }
    Ok(Contents::named(names))
}

pub fn extract(request: &Request) -> Result<()> {
    request.every_member()?;
    request.keeping_existing()?;
    let directory = request.directory()?;
    {
        let mount = Mount::of(request.archive)?;
        copy_tree(mount.path(), directory)?;
    }
    for path in tree::paths(directory)? {
        let mode = if path.is_dir() {
            DIRECTORY_MODE
        } else {
            FILE_MODE
        };
        fs::set_permissions(&path, Permissions::from_mode(mode))
            .with_context(|| format!("setting the mode of {}", path.display()))?;
    }
    Ok(())
}

fn copy_tree(source: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
    let entries = fs::read_dir(source).with_context(|| format!("listing {}", source.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("listing {}", source.display()))?;
        let (from, to) = (entry.path(), dest.join(entry.file_name()));
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from `udisksctl info --block-device /dev/loop0` for a partitioned image.
    const LOOP_DEVICE: &str = "\
/org/freedesktop/UDisks2/block_devices/loop0:
  org.freedesktop.UDisks2.Block:
    Device:                     /dev/loop0
    Symlinks:                   /dev/disk/by-id/a
                                /dev/disk/by-id/b

  org.freedesktop.UDisks2.Filesystem:
    MountPoints:
  org.freedesktop.UDisks2.PartitionTable:
    Partitions:                 /org/freedesktop/UDisks2/block_devices/loop0p1
    Type:                       dos
";

    fn values<'i>(info: &'i Info, section: &str, name: &str) -> Option<&'i [String]> {
        info.get(section)
            .and_then(|properties| properties.get(name))
            .map(Vec::as_slice)
    }

    #[test]
    fn sections_are_listed() {
        let info = parse_info(LOOP_DEVICE).expect("valid info");
        assert_eq!(
            info.keys().map(String::as_str).collect::<Vec<_>>(),
            ["org.freedesktop.UDisks2.Block", FILESYSTEM, PARTITION_TABLE]
        );
    }

    #[test]
    fn property_value_is_trimmed() {
        let info = parse_info(LOOP_DEVICE).expect("valid info");
        assert_eq!(
            values(&info, PARTITION_TABLE, "Type"),
            Some(["dos".to_owned()].as_slice())
        );
    }

    #[test]
    fn continuation_is_appended() {
        let info = parse_info(LOOP_DEVICE).expect("valid info");
        assert_eq!(
            values(&info, "org.freedesktop.UDisks2.Block", "Symlinks"),
            Some(
                [
                    "/dev/disk/by-id/a".to_owned(),
                    "/dev/disk/by-id/b".to_owned()
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn empty_value_is_not_appended() {
        let info = parse_info(LOOP_DEVICE).expect("valid info");
        assert!(info[FILESYSTEM].is_empty());
    }

    #[test]
    fn blank_lines_are_skipped() {
        let info = parse_info("  Section:\n    Name: a\n\n      \n      b\n").expect("valid info");
        assert_eq!(
            values(&info, "Section", "Name"),
            Some(["a".to_owned(), "b".to_owned()].as_slice())
        );
    }

    #[test]
    fn under_indented_lines_are_ignored() {
        let info = parse_info(" Ignored: x\n  Section:\n    Name: a\n").expect("valid info");
        assert_eq!(info.keys().collect::<Vec<_>>(), ["Section"]);
    }

    #[test]
    fn section_name_drops_trailing_colons() {
        let info = parse_info("   Section::  \n").expect("valid info");
        assert!(info.contains_key("Section"));
    }

    #[test]
    fn repeated_section_is_emptied() {
        let info = parse_info("  Section:\n    Name: a\n  Section:\n").expect("valid info");
        assert!(info["Section"].is_empty());
    }

    #[test]
    fn property_splits_on_first_colon() {
        let info = parse_info("  Section:\n    Name: a: b\n").expect("valid info");
        assert_eq!(
            values(&info, "Section", "Name"),
            Some(["a: b".to_owned()].as_slice())
        );
    }

    #[test]
    fn property_without_colon_is_an_error() {
        let error = parse_info("  Section:\n    Name\n").expect_err("invalid info");
        assert_eq!(error.to_string(), UNEXPECTED_OUTPUT);
    }

    #[test]
    fn property_without_section_is_an_error() {
        let error = parse_info("    Name: a\n").expect_err("invalid info");
        assert_eq!(error.to_string(), UNEXPECTED_OUTPUT);
    }

    #[test]
    fn continuation_without_property_is_an_error() {
        let error = parse_info("  Section:\n      a\n").expect_err("invalid info");
        assert_eq!(error.to_string(), UNEXPECTED_OUTPUT);
    }

    // A section header forgets the property before it.
    #[test]
    fn continuation_after_section_is_an_error() {
        assert!(parse_info("  Section:\n    Name: a\n  Other:\n      b\n").is_err());
    }
}
