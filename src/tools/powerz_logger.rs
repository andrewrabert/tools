//! Log readings from a ChargerLAB POWER-Z KM003C USB meter through the `powerz` hwmon driver.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use clap::builder::styling::{AnsiColor, Style};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::tools::Tool;

/// Directory of the devices bound to the `powerz` kernel driver.
const DRIVER_DIR: &str = "/sys/bus/usb/drivers/powerz";

#[derive(ClapArgs)]
#[command(about = "Print the voltage, current, and power a POWER-Z KM003C USB meter reads")]
pub struct PowerzLogger {
    /// Seconds between readings
    #[arg(short = 'n', long, value_name = "SECONDS", default_value_t = 1)]
    interval: u64,
    /// Also write each reading as a JSON line to FILE
    #[arg(short, long, value_name = "FILE", conflicts_with = "input")]
    output: Option<PathBuf>,
    /// Print the readings in a JSON-lines FILE that --output wrote, then exit
    #[arg(short, long, value_name = "FILE")]
    input: Option<PathBuf>,
}

impl Tool for PowerzLogger {
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

/// Sensor values keyed by hwmon label (`VBUS`, `IBUS`, ...), in milli-units.
type State = BTreeMap<String, i64>;

/// One reading as a JSON line. Fields are in sorted order.
#[derive(Serialize, Deserialize)]
struct Record {
    state: State,
    timestamp: Timestamp,
}

fn run(args: PowerzLogger) -> Result<()> {
    if let Some(input) = &args.input {
        return replay(input);
    }
    let hwmon = find_device(Path::new(DRIVER_DIR))?;
    let mut output = args
        .output
        .as_ref()
        .map(|path| {
            File::create(path)
                .map(BufWriter::new)
                .with_context(|| format!("creating {}", path.display()))
        })
        .transpose()?;
    loop {
        let record = Record {
            timestamp: Timestamp::now(),
            state: read_device(&hwmon)?,
        };
        anstream::println!("{}", format_human(&record)?);
        if let (Some(writer), Some(path)) = (&mut output, &args.output) {
            serde_json::to_writer(&mut *writer, &record)
                .map_err(anyhow::Error::from)
                .and_then(|()| Ok(writeln!(writer)?))
                .and_then(|()| Ok(writer.flush()?))
                .with_context(|| format!("writing {}", path.display()))?;
        }
        thread::sleep(Duration::from_secs(args.interval));
    }
}

fn replay(path: &Path) -> Result<()> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    for (number, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("reading {}", path.display()))?;
        let record: Record = serde_json::from_str(&line)
            .with_context(|| format!("parsing {} line {}", path.display(), number + 1))?;
        anstream::println!("{}", format_human(&record)?);
    }
    Ok(())
}

/// Find the hwmon directory of the one device bound to the driver in `driver_dir`.
fn find_device(driver_dir: &Path) -> Result<PathBuf> {
    let entries = fs::read_dir(driver_dir)
        .with_context(|| format!("device not found: reading {}", driver_dir.display()))?;
    let mut matches = Vec::new();
    for entry in entries {
        let hwmon = entry?.path().join("hwmon");
        if !hwmon.is_dir() {
            continue;
        }
        let mut children = fs::read_dir(&hwmon)
            .with_context(|| format!("reading {}", hwmon.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        match children.pop() {
            Some(child) if children.is_empty() => matches.push(child),
            _ => bail!("expected exactly one hwmon device in {}", hwmon.display()),
        }
    }
    match matches.pop() {
        Some(device) if matches.is_empty() => Ok(device),
        Some(_) => bail!("multiple devices found"),
        None => bail!("device not found"),
    }
}

/// Read each sensor's label and input from the hwmon directory `hwmon`.
fn read_device(hwmon: &Path) -> Result<State> {
    let mut labels = BTreeMap::new();
    let mut inputs = BTreeMap::new();
    for entry in fs::read_dir(hwmon).with_context(|| format!("reading {}", hwmon.display()))? {
        let path = entry?.path();
        let Some((sensor, kind)) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.split_once('_'))
        else {
            continue;
        };
        if !matches!(kind, "label" | "input") || !path.is_file() {
            continue;
        }
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let value = text.trim_end_matches('\n');
        if value.contains('\n') {
            bail!("expected one line in {}", path.display());
        }
        let sensor = sensor.to_owned();
        if kind == "label" {
            labels.insert(sensor, value.to_owned());
        } else {
            let value = value
                .parse::<i64>()
                .with_context(|| format!("parsing {}", path.display()))?;
            inputs.insert(sensor, value);
        }
    }
    let mut state = State::new();
    for (sensor, label) in labels {
        let value = inputs
            .remove(&sensor)
            .with_context(|| format!("sensor {sensor} has a label but no input"))?;
        if state.insert(label.clone(), value).is_some() {
            bail!("duplicate sensor label {label}");
        }
    }
    Ok(state)
}

/// Format a reading as local time, bus voltage, bus current, and power.
fn format_human(record: &Record) -> Result<String> {
    let milli = |label: &str| {
        record
            .state
            .get(label)
            .map(|&value| value as f64 * 0.001)
            .with_context(|| format!("reading has no {label}"))
    };
    let volts = milli("VBUS")?;
    let amps = milli("IBUS")?.abs();
    let time = record
        .timestamp
        .to_zoned(TimeZone::system())
        .strftime("%Y-%m-%d %H:%M:%S");
    let field = |color: AnsiColor, key: &str, value: f64| {
        let style = Style::new().bold().fg_color(Some(color.into()));
        format!("{style}{key}:{style:#} {value:.3}")
    };
    Ok([
        time.to_string(),
        field(AnsiColor::Green, "VBUS (V)", volts),
        field(AnsiColor::Red, "IBUS (A)", amps),
        field(AnsiColor::Blue, "PWR (W)", volts * amps),
    ]
    .join("   "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, contents: &str) {
        fs::write(dir.join(name), contents).unwrap();
    }

    #[test]
    fn reads_labeled_inputs() {
        let hwmon = tempfile::tempdir().unwrap();
        write(hwmon.path(), "in0_label", "VBUS\n");
        write(hwmon.path(), "in0_input", "5012\n");
        write(hwmon.path(), "in0_average", "5000\n");
        write(hwmon.path(), "curr1_label", "IBUS\n");
        write(hwmon.path(), "curr1_input", "-1500\n");
        write(hwmon.path(), "name", "powerz\n");
        let state = read_device(hwmon.path()).unwrap();
        assert_eq!(
            state,
            State::from([("IBUS".to_owned(), -1500), ("VBUS".to_owned(), 5012)])
        );
    }

    #[test]
    fn finds_the_one_device() {
        let driver = tempfile::tempdir().unwrap();
        let device = driver.path().join("1-2:1.0/hwmon/hwmon7");
        fs::create_dir_all(&device).unwrap();
        fs::create_dir(driver.path().join("module")).unwrap();
        write(driver.path(), "bind", "");
        assert_eq!(find_device(driver.path()).unwrap(), device);
    }

    #[test]
    fn rejects_multiple_devices() {
        let driver = tempfile::tempdir().unwrap();
        fs::create_dir_all(driver.path().join("1-2:1.0/hwmon/hwmon7")).unwrap();
        fs::create_dir_all(driver.path().join("1-3:1.0/hwmon/hwmon8")).unwrap();
        let error = find_device(driver.path()).unwrap_err();
        assert_eq!(error.to_string(), "multiple devices found");
    }

    #[test]
    fn record_round_trips_python_logs() {
        let line = r#"{"state":{"IBUS":-1500,"VBUS":5012},"timestamp":"2024-01-02T03:04:05.123456+00:00"}"#;
        let record: Record = serde_json::from_str(line).unwrap();
        assert_eq!(
            serde_json::to_string(&record).unwrap(),
            r#"{"state":{"IBUS":-1500,"VBUS":5012},"timestamp":"2024-01-02T03:04:05.123456Z"}"#
        );
    }

    #[test]
    fn formats_power() {
        let record = Record {
            state: State::from([("IBUS".to_owned(), -1500), ("VBUS".to_owned(), 5012)]),
            timestamp: Timestamp::UNIX_EPOCH,
        };
        let line = format_human(&record).unwrap();
        let plain = anstream::adapter::strip_str(&line).to_string();
        let fields: Vec<_> = plain.split("   ").skip(1).collect();
        assert_eq!(
            fields,
            ["VBUS (V): 5.012", "IBUS (A): 1.500", "PWR (W): 7.518"]
        );
    }
}
