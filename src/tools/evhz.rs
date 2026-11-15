//! Measure the polling rate of evdev input devices.
//!
//! Ported from Ian Kelling's `evhz` (Apache-2.0, <https://git.iankelling.org/evhz>).

use std::fs::{self, File};
use std::io::{self, ErrorKind, Read};
use std::os::fd::{AsFd, AsRawFd};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use signal_hook::consts::SIGINT;

use crate::tools::Tool;

const INPUT_DIR: &str = "/dev/input";
const NAME_LEN: usize = 128;
/// Samples the running average covers.
const SAMPLES: usize = 64;

const EV_REL: u16 = 0x02;
const EV_ABS: u16 = 0x03;

// EVIOCGNAME(len) from <linux/input.h>.
nix::ioctl_read_buf!(eviocgname, b'E', 0x06, u8);

/// `struct input_event` from <linux/input.h>, declared here so the tool builds on every platform.
#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    time: libc::timeval,
    type_: u16,
    code: u16,
    value: i32,
}

#[derive(ClapArgs)]
#[command(about = "Measure the polling rate of mice and other evdev devices")]
pub struct Evhz {
    /// Report only the final averages, not every event
    #[arg(short = 'n', long)]
    nonverbose: bool,
}

impl Tool for Evhz {
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

fn run(args: Evhz) -> Result<()> {
    let verbose = !args.nonverbose;
    let mut devices = open_devices(Path::new(INPUT_DIR))?;
    if devices.is_empty() {
        bail!("no readable event devices in {INPUT_DIR}; try running as root");
    }
    let interrupted = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGINT, Arc::clone(&interrupted))
        .context("installing the SIGINT handler")?;

    println!("Press CTRL-C to exit.\n");
    if verbose {
        for device in &devices {
            println!("{}: {}", device.event, device.name);
        }
    }

    while !interrupted.load(Ordering::Relaxed) {
        let mut fds: Vec<PollFd> = devices
            .iter()
            .map(|device| PollFd::new(device.file.as_fd(), PollFlags::POLLIN))
            .collect();
        match poll(&mut fds, PollTimeout::NONE) {
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => continue,
            Err(error) => return Err(error).context("polling the event devices"),
        }
        let ready: Vec<bool> = fds.iter().map(|fd| fd.any().unwrap_or(false)).collect();
        for (device, ready) in devices.iter_mut().zip(ready) {
            if !ready {
                continue;
            }
            let Some(event) = device.read_event()? else {
                continue;
            };
            if event.type_ != EV_REL && event.type_ != EV_ABS {
                continue;
            }
            let micros = u64::try_from(event.time.tv_sec).unwrap_or(0) * 1_000_000
                + u64::try_from(event.time.tv_usec).unwrap_or(0);
            if let Some(hz) = device.rate.record(micros)
                && verbose
            {
                println!(
                    "{}: Latest {hz:5}Hz, Average {:5}Hz",
                    device.name,
                    device.rate.average()
                );
            }
        }
    }

    for device in &devices {
        let average = device.rate.average();
        if average != 0 {
            println!("\nAverage for {}: {average:5}Hz", device.name);
        }
    }
    Ok(())
}

struct Device {
    /// The device file name, like `event3`.
    event: String,
    name: String,
    file: File,
    rate: Rate,
}

impl Device {
    fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let mut name = [0u8; NAME_LEN];
        // SAFETY: EVIOCGNAME writes at most `name.len()` bytes into `name`.
        let len = unsafe { eviocgname(file.as_raw_fd(), &mut name) }.map_err(io::Error::from)?;
        let len = usize::try_from(len).unwrap_or(0).min(NAME_LEN);
        let name = name[..len].split(|&byte| byte == 0).next().unwrap_or(&[]);
        Ok(Self {
            event: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            name: String::from_utf8_lossy(name).into_owned(),
            file,
            rate: Rate::default(),
        })
    }

    /// Read one event; `None` when the device went away or has nothing to read.
    fn read_event(&mut self) -> Result<Option<InputEvent>> {
        let mut event = unsafe { std::mem::zeroed::<InputEvent>() };
        // SAFETY: InputEvent is plain old data, so any bytes are a valid value.
        let buffer = unsafe {
            std::slice::from_raw_parts_mut((&raw mut event).cast::<u8>(), size_of::<InputEvent>())
        };
        match self.file.read(buffer) {
            Ok(bytes) if bytes == buffer.len() => Ok(Some(event)),
            Ok(_) => Ok(None),
            Err(error)
                if matches!(error.kind(), ErrorKind::Interrupted | ErrorKind::WouldBlock) =>
            {
                Ok(None)
            }
            Err(error) => Err(error).with_context(|| format!("reading {}", self.event)),
        }
    }
}

/// Open every `/dev/input/eventN` that can be read, in numeric order.
fn open_devices(dir: &Path) -> Result<Vec<Device>> {
    let mut paths: Vec<(u32, PathBuf)> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let number = path
                .file_name()?
                .to_str()?
                .strip_prefix("event")?
                .parse()
                .ok()?;
            Some((number, path))
        })
        .collect();
    paths.sort_unstable();
    let mut devices = Vec::new();
    for (_, path) in paths {
        match Device::open(&path) {
            Ok(device) => devices.push(device),
            Err(error) if error.kind() == ErrorKind::PermissionDenied => {}
            Err(error) => return Err(error).with_context(|| format!("opening {}", path.display())),
        }
    }
    Ok(devices)
}

/// The event rate of one device, averaged over the last [`SAMPLES`] intervals.
struct Rate {
    samples: [u64; SAMPLES],
    count: usize,
    previous: Option<u64>,
}

impl Default for Rate {
    fn default() -> Self {
        Self {
            samples: [0; SAMPLES],
            count: 0,
            previous: None,
        }
    }
}

impl Rate {
    /// Record an event at `micros` and return its rate relative to the previous event.
    fn record(&mut self, micros: u64) -> Option<u64> {
        let previous = self.previous.replace(micros)?;
        let elapsed = micros.checked_sub(previous)?;
        if elapsed == 0 {
            return None;
        }
        let hz = 1_000_000 / elapsed;
        if hz == 0 {
            return None;
        }
        self.samples[self.count % SAMPLES] = hz;
        self.count += 1;
        Some(hz)
    }

    fn average(&self) -> u64 {
        let filled = self.count.min(SAMPLES);
        if filled == 0 {
            return 0;
        }
        self.samples[..filled].iter().sum::<u64>() / filled as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_event_has_no_rate() {
        let mut rate = Rate::default();
        assert_eq!(rate.record(1_000), None);
        assert_eq!(rate.average(), 0);
    }

    #[test]
    fn rate_is_the_inverse_of_the_interval() {
        let mut rate = Rate::default();
        rate.record(0);
        assert_eq!(rate.record(1_000), Some(1_000));
        assert_eq!(rate.record(3_000), Some(500));
        assert_eq!(rate.average(), 750);
    }

    #[test]
    fn same_timestamp_is_skipped() {
        let mut rate = Rate::default();
        rate.record(0);
        assert_eq!(rate.record(0), None);
        assert_eq!(rate.record(1_000), Some(1_000));
    }

    #[test]
    fn average_covers_the_last_samples() {
        let mut rate = Rate::default();
        let mut now = 0;
        rate.record(now);
        for _ in 0..SAMPLES {
            now += 2_000;
            rate.record(now);
        }
        assert_eq!(rate.average(), 500);
        for _ in 0..SAMPLES {
            now += 1_000;
            rate.record(now);
        }
        assert_eq!(rate.average(), 1_000);
    }
}
