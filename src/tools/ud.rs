use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Output, Stdio};

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use ratatui::text::Line;

use crate::picker::{self, Matching};
use crate::tools::Tool;
use crate::tools::cdemu_tool::{self, Device, ImagePath, Images, SrDevice};

/// The width of the action column in the picker.
const ACTION_WIDTH: usize = 8;
const CDEMU_EXTENSIONS: [&str; 7] = ["ccd", "cue", "iso", "mds", "mdf", "nrg", "toc"];
const TOC_EXTENSION: &str = "toc";
const MOUNTS: &str = "/proc/mounts";
const SSHFS_TYPE: &str = "fuse.sshfs";

/// The fields asked of `udiskie-info`, printed tab separated.
const UDISKIE_COLUMNS: [&str; 42] = [
    "autoclear",
    "device_file",
    "device_id",
    "device_presentation",
    "device_size",
    "drive_label",
    "drive_model",
    "drive_vendor",
    "has_media",
    "id_label",
    "id_type",
    "id_usage",
    "id_uuid",
    "in_use",
    "is_block",
    "is_crypto",
    "is_detachable",
    "is_drive",
    "is_ejectable",
    "is_external",
    "is_filesystem",
    "is_ignored",
    "is_loop",
    "is_luks",
    "is_luks_cleartext",
    "is_mounted",
    "is_partition",
    "is_partition_table",
    "is_systeminternal",
    "is_toplevel",
    "is_unlocked",
    "loop_file",
    "mount_path",
    "mount_paths",
    "setup_by_uid",
    "should_automount",
    "symlinks",
    "ui_device_label",
    "ui_device_presentation",
    "ui_id_label",
    "ui_id_uuid",
    "ui_label",
];

#[derive(ClapArgs)]
#[command(about = "Mount disk images, or pick a device, disc image, or sshfs mount to (un)mount")]
pub struct Ud {
    /// Print udiskie's view of every device as JSON
    #[arg(long)]
    json: bool,
    /// Mount options passed to udiskie-mount
    #[arg(short, long, default_value = "")]
    options: String,
    /// Images to mount. Several must all be .toc files
    #[arg(value_name = "IMAGE")]
    images: Vec<PathBuf>,
}

impl Tool for Ud {
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

type UdiskieDevice = BTreeMap<String, String>;

enum Action {
    Mount,
    Unmount,
    Detach,
}

impl Action {
    fn name(&self) -> &'static str {
        match self {
            Action::Mount => "mount",
            Action::Unmount => "unmount",
            Action::Detach => "detach",
        }
    }
}

enum Target {
    Udiskie { device_file: String },
    Cdemu { device: SrDevice },
    Sshfs { mount_point: String },
}

fn run(args: Ud) -> Result<ExitCode> {
    if args.json {
        let devices = udiskie_finish(udiskie_spawn()?)?;
        println!("{}", serde_json::to_string_pretty(&devices)?);
        return Ok(ExitCode::SUCCESS);
    }
    match args.images.as_slice() {
        [] => pick(&args.options),
        [image] => {
            if has_extension(image, &CDEMU_EXTENSIONS) {
                cdemu_load(&args.images)?;
            } else {
                udiskie_mount(image.as_os_str(), &args.options)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        images => {
            if let Some(image) = images
                .iter()
                .find(|image| !has_extension(image, &[TOC_EXTENSION]))
            {
                bail!(
                    "all files must be .{TOC_EXTENSION} when specifying multiple: {}",
                    image.display()
                );
            }
            cdemu_load(images)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn pick(options: &str) -> Result<ExitCode> {
    // udiskie is spawned first so it runs while the rest is gathered.
    let udiskie = udiskie_spawn()?;
    let cdemu = cdemu_tool::devices()?;
    let sshfs = sshfs_mount_points()?;
    let udiskie = udiskie_finish(udiskie)?;

    let mut items = BTreeMap::new();
    for device in udiskie {
        let field = |name: &str| device.get(name).map_or("", String::as_str);
        let (action, label) = if field("is_filesystem") == "True" {
            let action = if field("is_mounted") == "False" {
                Action::Mount
            } else {
                Action::Unmount
            };
            (
                action,
                format!("{} {}", field("ui_label"), field("mount_path")),
            )
        } else if field("is_loop") == "True" && field("is_toplevel") == "True" {
            (Action::Detach, field("ui_label").to_owned())
        } else {
            continue;
        };
        let key = item_key(&action, &label);
        let target = Target::Udiskie {
            device_file: field("device_file").to_owned(),
        };
        if items.insert(key.clone(), (action, target)).is_some() {
            bail!("udiskie listed {key:?} twice");
        }
    }
    for device in cdemu {
        let label = format!(
            "{}: {}",
            device.dev_sr(),
            cdemu_label(device.filenames().as_slice())
        );
        let key = item_key(&Action::Detach, &label);
        let target = Target::Cdemu {
            device: device.dev_sr().clone(),
        };
        items.insert(key, (Action::Detach, target));
    }
    for mount_point in sshfs {
        let key = item_key(&Action::Unmount, &mount_point);
        items.insert(key, (Action::Unmount, Target::Sshfs { mount_point }));
    }

    if items.is_empty() {
        return Ok(ExitCode::FAILURE);
    }
    let picked = {
        let lines: Vec<Line> = items.keys().map(|key| Line::raw(key.as_str())).collect();
        picker::pick(&lines, "", Matching::Exact)?
    };
    let Some((action, target)) = picked
        .and_then(|picked| picked.item)
        .and_then(|index| items.into_values().nth(index))
    else {
        return Ok(ExitCode::FAILURE);
    };
    match (action, target) {
        (Action::Mount, Target::Udiskie { device_file }) => {
            udiskie_mount(OsStr::new(&device_file), options)?
        }
        (Action::Unmount | Action::Detach, Target::Udiskie { device_file }) => status(
            Command::new("udiskie-umount").arg("-q").arg(&device_file),
            "udiskie-umount",
        )?,
        (_, Target::Sshfs { mount_point }) => status(
            Command::new("fusermount3")
                .args(["-quz", "--"])
                .arg(&mount_point),
            "fusermount3",
        )?,
        (_, Target::Cdemu { device }) => cdemu_tool::remove(&device)?,
    }
    Ok(ExitCode::SUCCESS)
}

/// The image name and directory when the images share one, else every path.
fn cdemu_label(filenames: &[ImagePath]) -> String {
    let paths: Vec<&Path> = filenames.iter().map(ImagePath::as_path).collect();
    let parents: BTreeSet<Option<&Path>> = paths.iter().map(|path| path.parent()).collect();
    match (parents.len(), parents.first()) {
        (1, Some(Some(parent))) => {
            let names: Vec<String> = paths
                .iter()
                .map(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default()
                })
                .collect();
            let names = match names.as_slice() {
                [name] => name.clone(),
                names => format!("{names:?}"),
            };
            format!("{names} ({})", parent.display())
        }
        _ => format!("{paths:?}"),
    }
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension().is_some_and(|extension| {
        extensions
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    })
}

fn udiskie_spawn() -> Result<Child> {
    let output = UDISKIE_COLUMNS
        .iter()
        .map(|column| format!("{{{column}}}"))
        .collect::<Vec<_>>()
        .join("\t");
    spawn_piped(
        Command::new("udiskie-info")
            .args(["--all", "--output"])
            .arg(output),
        "udiskie-info",
    )
}

fn udiskie_finish(child: Child) -> Result<Vec<UdiskieDevice>> {
    let output = finish(child, "udiskie-info")?;
    let stdout = String::from_utf8(output.stdout).context("udiskie-info printed non-UTF-8")?;
    Ok(stdout
        .lines()
        .map(|line| {
            UDISKIE_COLUMNS
                .iter()
                .zip(line.split('\t'))
                .map(|(column, value)| ((*column).to_owned(), value.to_owned()))
                .collect()
        })
        .collect())
}

fn udiskie_mount(device: &OsStr, options: &str) -> Result<()> {
    let mut command = Command::new("udiskie-mount");
    command.arg("-q");
    if !options.is_empty() {
        command.arg("--options").arg(options);
    }
    command.arg("--no-recursive").arg(device);
    status(&mut command, "udiskie-mount")
}

fn cdemu_load(images: &[PathBuf]) -> Result<()> {
    let device: Device = cdemu_tool::load(&Images::from_paths(images)?)?;
    println!("{}", serde_json::to_string_pretty(&device)?);
    Ok(())
}

/// The sshfs mount points in `/proc/mounts`.
fn sshfs_mount_points() -> Result<Vec<String>> {
    let mounts = fs::read_to_string(MOUNTS).with_context(|| format!("reading {MOUNTS}"))?;
    Ok(mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (_, mount_point, kind) = (fields.next()?, fields.next()?, fields.next()?);
            (kind == SSHFS_TYPE).then(|| unescape_mount_field(mount_point))
        })
        .collect())
}

/// Undoes the octal escapes (`\040` for a space) of a `/proc/mounts` field.
fn unescape_mount_field(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut unescaped = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let octal = bytes
            .get(index + 1..index + 4)
            .filter(|_| bytes[index] == b'\\')
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u8::from_str_radix(digits, 8).ok());
        match octal {
            Some(byte) => {
                unescaped.push(byte);
                index += 4;
            }
            None => {
                unescaped.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&unescaped).into_owned()
}

/// The line of an item in the picker, with the labels in one column.
fn item_key(action: &Action, label: &str) -> String {
    format!("{:<ACTION_WIDTH$}{label}", action.name())
}

fn spawn_piped(command: &mut Command, name: &str) -> Result<Child> {
    command
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {name}"))
}

fn finish(child: Child, name: &str) -> Result<Output> {
    let output = child
        .wait_with_output()
        .with_context(|| format!("waiting for {name}"))?;
    if !output.status.success() {
        bail!("{name} failed: {}", output.status);
    }
    Ok(output)
}

fn status(command: &mut Command, name: &str) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("running {name}"))?;
    if !status.success() {
        bail!("{name} failed: {status}");
    }
    Ok(())
}
