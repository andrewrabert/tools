use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub heads: u8,
    pub start: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Probe {
    pub has_size: bool,
    pub geometry: Option<Geometry>,
    pub floppy: bool,
    pub sector_size: Option<u32>,
}

impl Probe {
    pub fn is_disk_device(&self) -> bool {
        self.has_size || self.geometry.is_some()
    }

    pub fn is_partition(&self) -> bool {
        self.start_sector() != 0
    }

    /// Sectors on the disk before this partition, or zero when unknown:
    /// upstream's `partition_start_sector`.
    ///
    /// UPSTREAM-BUG (not reproduced): upstream reads `hd_geometry` whenever
    /// either `BLKGETSIZE` or `HDIO_GETGEO` succeeded. If only `BLKGETSIZE`
    /// did, as on a loop device, the struct was never filled in and upstream
    /// acts on whatever was on the stack. This port treats that case as a
    /// start sector and head count of zero.
    pub fn start_sector(&self) -> u64 {
        self.geometry.map_or(0, |geometry| geometry.start)
    }

    pub fn heads(&self) -> u16 {
        self.geometry
            .map_or(0, |geometry| u16::from(geometry.heads))
    }

    pub fn sector_size_or_default(&self) -> u64 {
        self.sector_size.map_or(512, u64::from)
    }
}

pub trait Device {
    fn read_at(&self, pos: u64, buf: &mut [u8]) -> io::Result<()>;

    fn write_at(&mut self, pos: u64, data: &[u8]) -> io::Result<()>;

    fn probe(&self) -> Probe;

    fn contains(&self, pos: u64, data: &[u8]) -> bool {
        let mut buf = vec![0; data.len()];
        self.read_at(pos, &mut buf).is_ok() && buf == data
    }
}

const BLKGETSIZE: libc::c_ulong = 0x1260;
const BLKSSZGET: libc::c_ulong = 0x1268;
const HDIO_GETGEO: libc::c_ulong = 0x0301;
/// `FDGETPRM` from `<linux/fd.h>`: `_IOR(2, 0x04, struct floppy_struct)` in
/// the generic ioctl encoding used by x86, ARM and RISC-V.
const FDGETPRM: libc::c_ulong =
    (2 << 30) | ((size_of::<FloppyStruct>() as libc::c_ulong) << 16) | (2 << 8) | 0x04;

#[repr(C)]
#[derive(Default)]
struct HdGeometry {
    heads: libc::c_uchar,
    sectors: libc::c_uchar,
    cylinders: libc::c_ushort,
    start: libc::c_ulong,
}

#[repr(C)]
struct FloppyStruct {
    size: libc::c_uint,
    sect: libc::c_uint,
    head: libc::c_uint,
    track: libc::c_uint,
    stretch: libc::c_uint,
    gap: libc::c_uchar,
    rate: libc::c_uchar,
    spec1: libc::c_uchar,
    fmt_gap: libc::c_uchar,
    name: *const libc::c_char,
}

/// Issues an ioctl that fills `arg`.
///
/// # Safety
///
/// `request` must be an ioctl that writes at most `size_of::<T>()` bytes
/// through its argument and reads nothing from it.
unsafe fn query<T>(file: &File, request: libc::c_ulong, arg: &mut T) -> io::Result<()> {
    // SAFETY: the pointer is valid and exclusively borrowed for the call,
    // and the caller vouches for how much the kernel writes through it.
    // The request parameter is an `int` on some C libraries.
    let status = unsafe { libc::ioctl(file.as_raw_fd(), request as _, std::ptr::from_mut(arg)) };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn probe(file: &File) -> Probe {
    let mut sectors: libc::c_long = 0;
    let mut geometry = HdGeometry::default();
    let mut floppy = std::mem::MaybeUninit::<FloppyStruct>::zeroed();
    let mut sector_size: libc::c_int = 0;
    // SAFETY: each request is paired with the type the kernel fills for it:
    // a `long`, a `struct hd_geometry`, a `struct floppy_struct`, an `int`.
    unsafe {
        Probe {
            has_size: query(file, BLKGETSIZE, &mut sectors).is_ok(),
            geometry: query(file, HDIO_GETGEO, &mut geometry)
                .ok()
                .map(|()| Geometry {
                    heads: geometry.heads,
                    start: geometry.start,
                }),
            floppy: query(file, FDGETPRM, &mut floppy).is_ok(),
            sector_size: query(file, BLKSSZGET, &mut sector_size)
                .ok()
                .map(|()| sector_size as u32),
        }
    }
}

pub struct FileDevice {
    file: File,
    probe: Probe,
}

impl FileDevice {
    pub fn open(path: &OsStr, writable: bool) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(writable).open(path)?;
        let probe = probe(&file);
        Ok(Self { file, probe })
    }
}

impl Device for FileDevice {
    /// Upstream refuses reads longer than `MAX_DATA_LEN` (10240 bytes) with
    /// a message on stderr; no read in the program is that long, so the
    /// check is not ported.
    fn read_at(&self, pos: u64, buf: &mut [u8]) -> io::Result<()> {
        self.file.read_exact_at(buf, pos)
    }

    fn write_at(&mut self, pos: u64, data: &[u8]) -> io::Result<()> {
        self.file.write_all_at(data, pos)
    }

    fn probe(&self) -> Probe {
        self.probe
    }
}

/// A device held in memory, for the tests. It behaves as an image file
/// does: reads past the end fail, writes past the end extend it.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryDevice {
    pub bytes: Vec<u8>,
    pub probe: Probe,
}

#[cfg(test)]
impl MemoryDevice {
    pub fn new(bytes: Vec<u8>, probe: Probe) -> Self {
        Self { bytes, probe }
    }

    fn range(pos: u64, len: usize) -> Option<std::ops::Range<usize>> {
        let start = usize::try_from(pos).ok()?;
        Some(start..start.checked_add(len)?)
    }
}

#[cfg(test)]
impl Device for MemoryDevice {
    fn read_at(&self, pos: u64, buf: &mut [u8]) -> io::Result<()> {
        if buf.is_empty() {
            return Ok(());
        }
        let source = Self::range(pos, buf.len())
            .and_then(|range| self.bytes.get(range))
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        buf.copy_from_slice(source);
        Ok(())
    }

    fn write_at(&mut self, pos: u64, data: &[u8]) -> io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        let range = Self::range(pos, data.len()).ok_or(io::ErrorKind::InvalidInput)?;
        if range.end > self.bytes.len() {
            self.bytes
                .try_reserve(range.end - self.bytes.len())
                .map_err(|_| io::ErrorKind::OutOfMemory)?;
            self.bytes.resize(range.end, 0);
        }
        self.bytes[range].copy_from_slice(data);
        Ok(())
    }

    fn probe(&self) -> Probe {
        self.probe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_reads_fail_past_the_end() {
        let device = MemoryDevice::new(vec![1, 2, 3], Probe::default());
        let mut two = [0; 2];
        assert!(device.read_at(1, &mut two).is_ok());
        assert_eq!(two, [2, 3]);
        assert!(device.read_at(2, &mut two).is_err());
        assert!(device.read_at(u64::MAX, &mut two).is_err());
        assert!(device.read_at(u64::MAX, &mut []).is_ok());
        assert!(device.contains(0, &[1, 2, 3]));
        assert!(!device.contains(0, &[1, 2, 3, 4]));
    }

    #[test]
    fn memory_writes_extend_with_zeros() {
        let mut device = MemoryDevice::new(vec![1], Probe::default());
        assert!(device.write_at(3, &[9, 9]).is_ok());
        assert_eq!(device.bytes, [1, 0, 0, 9, 9]);
        assert!(device.write_at(0, &[7]).is_ok());
        assert_eq!(device.bytes, [7, 0, 0, 9, 9]);
        assert!(device.write_at(100, &[]).is_ok());
        assert_eq!(device.bytes.len(), 5);
        assert!(device.write_at(u64::MAX, &[1]).is_err());
    }

    #[test]
    fn a_regular_file_is_no_device() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), [0; 16]).unwrap();
        let device = FileDevice::open(file.path().as_os_str(), false).unwrap();
        assert_eq!(device.probe(), Probe::default());
    }
}
