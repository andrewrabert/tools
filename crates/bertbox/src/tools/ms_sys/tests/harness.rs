//! Disk images and device profiles for the tests, and the means to say
//! what an image should hold after a call.
//!
//! A test puts an [`Image`] in a `MemoryDevice` that claims to be some
//! [`Profile`], calls the library, and compares the device with the image it
//! expects, built with [`patched`]. No process is started and no file is
//! opened.

use std::fmt::Debug;
use std::fs;
use std::io;
use std::path::Path;

use super::super::device::{Device, Geometry, MemoryDevice, Probe};
use super::super::payloads::MBR_DOS_F2;

/// The device name command lines use for the image.
pub const DEVICE: &str = "disk.img";
/// Size of every fixture: enough for the highest offset the library
/// touches.
pub const IMAGE_SIZE: usize = 128 * 1024;
/// The boot signature that ends a boot sector.
pub const SIGNATURE: &[u8] = &[0x55, 0xaa];

/// What a device claims to be, under a name for failure reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: &'static str,
    /// What the kernel would say about the device.
    pub probe: Probe,
}

impl Profile {
    /// A regular file: every device query fails.
    pub const FILE: Self = Self {
        name: "file",
        probe: Probe {
            has_size: false,
            geometry: None,
            floppy: false,
            sector_size: None,
        },
    };
    /// A whole hard disk.
    pub const DISK: Self = Self::FILE.named("disk").sized().at(0, 255);
    /// A partition starting at sector 2048.
    pub const PARTITION: Self = Self::DISK.named("partition").at(2048, 255);
    /// A floppy drive.
    pub const FLOPPY: Self = Self::DISK.named("floppy").at(0, 2).floppy();
    /// Answers the floppy query and no other.
    pub const FLOPPY_IOCTL_ONLY: Self = Self::FILE.named("floppy-ioctl-only").floppy();
    /// Has a geometry but no size.
    pub const GEOMETRY_ONLY: Self = Self::FILE.named("geometry-only").at(2048, 255);
    /// Has a size but no geometry, as a loop device does.
    pub const SIZE_ONLY: Self = Self::FILE.named("size-only").sized();
    pub const PARTITION_16_HEADS: Self = Self::DISK.named("partition-16-heads").at(63, 16);
    pub const PARTITION_0_HEADS: Self = Self::DISK.named("partition-0-heads").at(2048, 0);
    /// A partition whose start sector does not fit in 32 bits.
    pub const PARTITION_BEYOND_32_BITS: Self = Self::DISK
        .named("partition-beyond-32-bits")
        .at(0x1_2345_6789, 255);
    /// A partition of a disk with 4096-byte sectors.
    pub const PARTITION_4K: Self = Self::PARTITION
        .named("partition-4k-sectors")
        .sectors_of(4096);
    pub const PARTITION_2_BYTE_SECTORS: Self = Self::PARTITION
        .named("partition-2-byte-sectors")
        .sectors_of(2);
    pub const PARTITION_0_BYTE_SECTORS: Self = Self::PARTITION
        .named("partition-0-byte-sectors")
        .sectors_of(0);
    /// A floppy drive that reports a start sector.
    pub const FLOPPY_PARTITION: Self = Self::FLOPPY.named("floppy-partition").at(2048, 2);

    pub const fn named(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }

    /// With a size the kernel knows.
    pub const fn sized(mut self) -> Self {
        self.probe.has_size = true;
        self
    }

    /// With a geometry: the sector it starts at and its number of heads.
    pub const fn at(mut self, start: u64, heads: u8) -> Self {
        self.probe.geometry = Some(Geometry { heads, start });
        self
    }

    /// Answering floppy queries.
    pub const fn floppy(mut self) -> Self {
        self.probe.floppy = true;
        self
    }

    /// With a logical sector size the kernel knows.
    pub const fn sectors_of(mut self, bytes: u32) -> Self {
        self.probe.sector_size = Some(bytes);
        self
    }

    /// Every profile, for tests that hold on any device.
    pub fn all() -> Vec<Self> {
        vec![
            Self::FILE,
            Self::DISK,
            Self::PARTITION,
            Self::FLOPPY,
            Self::FLOPPY_IOCTL_ONLY,
            Self::GEOMETRY_ONLY,
            Self::SIZE_ONLY,
            Self::PARTITION_16_HEADS,
            Self::PARTITION_0_HEADS,
            Self::PARTITION_BEYOND_32_BITS,
            Self::PARTITION_4K,
            Self::PARTITION_2_BYTE_SECTORS,
            Self::PARTITION_0_BYTE_SECTORS,
            Self::FLOPPY_PARTITION,
        ]
    }
}

/// A named disk image.
#[derive(Clone, Debug)]
pub struct Image {
    /// Label in failure reports.
    pub name: String,
    pub bytes: Vec<u8>,
}

impl Image {
    pub fn new(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            bytes,
        }
    }

    /// One of the committed file system fixtures: `fat12`, `fat16`,
    /// `fat32`, `exfat` or `ntfs`.
    pub fn fixture(name: &str) -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("src/tools/ms_sys/testdata/{name}.img"));
        let bytes = fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(bytes.len(), IMAGE_SIZE);
        Self::new(name, bytes)
    }

    /// Nothing but zeros.
    pub fn zeros() -> Self {
        Self::new("zeros", vec![0; IMAGE_SIZE])
    }

    /// Zeros with a boot signature: an empty master boot record.
    pub fn signed() -> Self {
        Self::zeros().with("signature", 0x1fe, SIGNATURE)
    }

    /// This image with `data` placed at `offset`.
    pub fn with(&self, suffix: &str, offset: usize, data: &[u8]) -> Self {
        Self::new(
            format!("{}+{suffix}", self.name),
            patched(&self.bytes, &[(offset, data)]),
        )
    }

    /// A device holding a copy of this image.
    pub fn device(&self, profile: Profile) -> MemoryDevice {
        MemoryDevice::new(self.bytes.clone(), profile.probe)
    }
}

/// Starting images of every sort: real file systems, images built to reach
/// what a freshly formatted file system does not, and images too short to
/// hold a boot sector.
pub fn images() -> Vec<Image> {
    let signed = Image::signed();
    let mut state = 0x6d73_2d73_7973_u64;
    let noise = (0..IMAGE_SIZE)
        .map(|_| {
            // A linear congruential generator; any fixed bytes would do.
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 56) as u8
        })
        .collect();
    let mut images: Vec<Image> = ["fat12", "fat16", "fat32", "exfat", "ntfs"]
        .map(Image::fixture)
        .to_vec();
    images.extend([
        signed
            .with(
                "partition-table",
                0x1be,
                &[0x80, 0x20, 0x21, 0x00, 0x0c, 0xfe, 0xff, 0xff],
            )
            .with(
                "disk-signature",
                0x1b8,
                &[0x78, 0x56, 0x34, 0x12, 0x5a, 0x5a],
            ),
        signed.with("lilo", 0x6, b"LILO"),
        signed.with("dos-f2", 0x0, MBR_DOS_F2),
        signed.with("copy-protect", 0x1bc, &[0x01, 0x02]),
        Image::fixture("fat16").with("mswin", 0x3, b"MSWIN4.1"),
        // FAT16 and FAT32 at once, as far as this program can tell.
        Image::fixture("fat16").with("fat32", 0x52, b"FAT32   "),
        Image::fixture("fat32").with("exfat", 0x3, b"EXFAT   "),
        Image::new("noise", noise),
        Image::new("short-511", vec![0; 511]),
        Image::new("short-100", vec![0x55; 100]),
        Image::new("empty", Vec::new()),
        signed,
        Image::zeros(),
    ]);
    images
}

/// The image of that name among [`images`].
pub fn image(name: &str) -> Image {
    images()
        .into_iter()
        .find(|image| image.name == name)
        .unwrap_or_else(|| panic!("no image called {name}"))
}

/// `before` with each `(offset, data)` written to it in turn. A write past
/// the end extends the image with zeros, as it does a file.
pub fn patched(before: &[u8], patches: &[(usize, &[u8])]) -> Vec<u8> {
    let mut bytes = before.to_vec();
    for &(offset, data) in patches {
        let end = offset + data.len();
        if end > bytes.len() && !data.is_empty() {
            bytes.resize(end, 0);
        }
        if !data.is_empty() {
            bytes[offset..end].copy_from_slice(data);
        }
    }
    bytes
}

/// Panics unless the images are the same, naming the first byte that
/// differs: printing both in full would bury it.
#[track_caller]
pub fn assert_image(actual: &[u8], expected: &[u8], context: impl Debug) {
    if actual == expected {
        return;
    }
    let shared = actual.len().min(expected.len());
    let at = (0..shared)
        .find(|&at| actual[at] != expected[at])
        .unwrap_or(shared);
    let window = |bytes: &[u8]| bytes[at..bytes.len().min(at + 16)].to_vec();
    panic!(
        "{context:?}: the image differs at {at:#x}\n  holds:    {:02x?} ({} bytes in all)\n  expected: {:02x?} ({} bytes in all)",
        window(actual),
        actual.len(),
        window(expected),
        expected.len(),
    );
}

/// Panics unless the image is exactly as it was.
#[track_caller]
pub fn assert_untouched(device: &MemoryDevice, image: &Image, context: impl Debug) {
    assert_image(&device.bytes, &image.bytes, context);
}

/// A device on which writes at some offsets fail.
pub struct Faulty {
    pub inner: MemoryDevice,
    fails: fn(u64) -> bool,
}

impl Faulty {
    pub fn new(image: &Image, profile: Profile, fails: fn(u64) -> bool) -> Self {
        Self {
            inner: image.device(profile),
            fails,
        }
    }
}

impl Device for Faulty {
    fn read_at(&self, pos: u64, buf: &mut [u8]) -> io::Result<()> {
        self.inner.read_at(pos, buf)
    }

    fn write_at(&mut self, pos: u64, data: &[u8]) -> io::Result<()> {
        if (self.fails)(pos) {
            Err(io::ErrorKind::PermissionDenied.into())
        } else {
            self.inner.write_at(pos, data)
        }
    }

    fn probe(&self) -> Probe {
        self.inner.probe()
    }
}
