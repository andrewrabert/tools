//! The domain types (device ids, image paths, `Device`, `Devices`).
use std::collections::BTreeMap;
use std::collections::btree_map;
use std::convert::Infallible;
use std::fmt;
use std::io;
use std::path::{self, Path, PathBuf};
use std::str::FromStr;

use anyhow::Result;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize, Serializer};

use crate::tools::cdemu_tool::unit::UnitName;

/// An sr (block) device path, such as `/dev/sr0`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(transparent)]
pub struct SrDevice(String);

/// An sg (generic SCSI) device path, such as `/dev/sg0`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct SgDevice(String);

impl From<String> for SrDevice {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl FromStr for SrDevice {
    type Err = Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.to_owned()))
    }
}

impl fmt::Display for SrDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for SgDevice {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl FromStr for SgDevice {
    type Err = Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.to_owned()))
    }
}

impl fmt::Display for SgDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An absolute path to a disc image.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ImagePath(PathBuf);

impl ImagePath {
    pub fn new(path: &Path) -> io::Result<Self> {
        Ok(Self(path::absolute(path)?))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// The error for an empty image collection.
#[derive(Debug)]
pub struct EmptyImages;

impl fmt::Display for EmptyImages {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("at least one image is required")
    }
}

impl std::error::Error for EmptyImages {}

/// A non-empty collection of disc images.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "Vec<ImagePath>", into = "Vec<ImagePath>")]
pub struct Images(Vec<ImagePath>);

impl Images {
    /// Errors if empty.
    pub fn new(images: Vec<ImagePath>) -> Result<Self, EmptyImages> {
        if images.is_empty() {
            return Err(EmptyImages);
        }
        Ok(Self(images))
    }

    pub fn from_paths<I: IntoIterator<Item = impl AsRef<Path>>>(paths: I) -> Result<Self> {
        let images = paths
            .into_iter()
            .map(|path| ImagePath::new(path.as_ref()))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self::new(images)?)
    }

    pub fn as_slice(&self) -> &[ImagePath] {
        &self.0
    }
}

impl TryFrom<Vec<ImagePath>> for Images {
    type Error = EmptyImages;

    fn try_from(images: Vec<ImagePath>) -> Result<Self, Self::Error> {
        Self::new(images)
    }
}

impl From<Images> for Vec<ImagePath> {
    fn from(images: Images) -> Self {
        images.0
    }
}

/// The address of the private session bus a unit's daemon listens on, for reaching it later.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct BusAddress(String);

impl From<String> for BusAddress {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Device {
    dbus_session_bus_address: BusAddress,
    dev_sg: SgDevice,
    dev_sr: SrDevice,
    filenames: Images,
    name: UnitName,
}

impl Device {
    pub fn new(
        dbus_session_bus_address: BusAddress,
        dev_sr: SrDevice,
        dev_sg: SgDevice,
        filenames: Images,
        name: UnitName,
    ) -> Self {
        Self {
            dbus_session_bus_address,
            dev_sg,
            dev_sr,
            filenames,
            name,
        }
    }

    pub fn dev_sr(&self) -> &SrDevice {
        &self.dev_sr
    }

    pub fn filenames(&self) -> &Images {
        &self.filenames
    }

    pub fn name(&self) -> &UnitName {
        &self.name
    }
}

/// The error for two records claiming the same sr path.
#[derive(Debug)]
pub struct DuplicateDevice {
    dev_sr: SrDevice,
    records: Vec<PathBuf>,
}

impl fmt::Display for DuplicateDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is recorded more than once:", self.dev_sr)?;
        for record in &self.records {
            write!(f, "\n  {}", record.display())?;
        }
        Ok(())
    }
}

impl std::error::Error for DuplicateDevice {}

/// Devices keyed by sr path; construction fails if two records claim one.
pub struct Devices(BTreeMap<SrDevice, Device>);

impl Devices {
    /// Each device paired with the path of the record it was read from.
    pub fn new(
        records: impl IntoIterator<Item = (PathBuf, Device)>,
    ) -> Result<Self, DuplicateDevice> {
        let mut by_sr: BTreeMap<SrDevice, Vec<(PathBuf, Device)>> = BTreeMap::new();
        for (path, device) in records {
            by_sr
                .entry(device.dev_sr.clone())
                .or_default()
                .push((path, device));
        }
        let mut map = BTreeMap::new();
        for (dev_sr, mut records) in by_sr {
            if records.len() > 1 {
                return Err(DuplicateDevice {
                    dev_sr,
                    records: records.into_iter().map(|(path, _)| path).collect(),
                });
            }
            let (_, device) = records.remove(0);
            map.insert(dev_sr, device);
        }
        Ok(Self(map))
    }

    pub fn get(&self, dev_sr: &SrDevice) -> Option<&Device> {
        self.0.get(dev_sr)
    }

    pub fn find_by_name(&self, name: &UnitName) -> Option<&Device> {
        self.0.values().find(|device| &device.name == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Device> {
        self.0.values()
    }
}

impl IntoIterator for Devices {
    type Item = Device;
    type IntoIter = btree_map::IntoValues<SrDevice, Device>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_values()
    }
}

impl Serialize for Devices {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for device in self.0.values() {
            seq.serialize_element(device)?;
        }
        seq.end()
    }
}
