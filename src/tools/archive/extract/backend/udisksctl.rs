use std::collections::BTreeMap;
use std::fs::{self, Permissions};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use nix::unistd::Uid;
use tempfile::TempDir;

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

type Info = BTreeMap<String, BTreeMap<String, Vec<String>>>;

fn info(device: &str) -> Result<Info> {
    let output = process::capture(
        Command::new("udisksctl")
            .args(["info", "--block-device"])
            .arg(device),
    )?;
    let output = String::from_utf8(output).context("udisksctl info is not valid UTF-8")?;

    let mut info = Info::new();
    let mut section = None;
    let mut property = None;
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let indent = line.len() - line.trim_start().len();
        let text = line.trim();
        if indent < SECTION_INDENT {
            continue;
        }
        if indent < PROPERTY_INDENT {
            let name = text.trim_end_matches(':').to_owned();
            info.insert(name.clone(), BTreeMap::new());
            section = Some(name);
            property = None;
            continue;
        }
        let properties = section
            .as_ref()
            .and_then(|section| info.get_mut(section))
            .context("unexpected udisksctl info output")?;
        let value = if indent == PROPERTY_INDENT {
            let (name, value) = text
                .split_once(':')
                .context("unexpected udisksctl info output")?;
            property = Some(name.to_owned());
            value.trim()
        } else {
            text
        };
        let name = property
            .clone()
            .context("unexpected udisksctl info output")?;
        if !value.is_empty() {
            properties.entry(name).or_default().push(value.to_owned());
        }
    }
    Ok(info)
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
