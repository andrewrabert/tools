use std::io;

use super::device::Device;
use super::exfat;
use super::identify::{
    self, OemIdUnsuitable, Unsuitable, is_fat16_fs, is_fat32_fs, is_ntfs_fs, sanity_check,
};
use super::record::{Family, Kind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heads {
    Kernel,
    Given(u16),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub record: Option<Kind>,
    pub force: bool,
    pub wipe_label: bool,
    pub partition: Option<Heads>,
    pub bytes_per_sector: u64,
    pub oem_id: Option<Vec<u8>>,
    pub disk_signature: Option<u32>,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            record: None,
            force: false,
            wipe_label: false,
            partition: None,
            bytes_per_sector: 512,
            oem_id: None,
            disk_signature: None,
        }
    }
}

impl Request {
    pub fn writes_nothing(&self) -> bool {
        self.record.is_none()
            && self.partition.is_none()
            && self.oem_id.is_none()
            && self.disk_signature.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Record { kind: Kind, reason: Unsuitable },
    OemId(OemIdUnsuitable),
    DiskSignature(Unsuitable),
    PartitionWithoutFileSystem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotWritten;

pub type Written = Result<(), NotWritten>;

fn written(result: io::Result<()>) -> Written {
    result.map_err(|_| NotWritten)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartitionReport {
    pub start_sector: u64,
    pub heads: Option<Result<u16, NotWritten>>,
    pub drive_ids: Vec<(Family, Written)>,
    pub exfat_checksum: Option<Written>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordReport {
    pub kind: Kind,
    pub written: Written,
    pub exfat_checksum: Option<Written>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub partition: Option<Result<PartitionReport, NotWritten>>,
    pub record: Option<RecordReport>,
    pub oem_id: Option<Written>,
    pub disk_signature: Option<Written>,
}

impl Report {
    /// UPSTREAM-BUG: once the start sector is written, a failure to write
    /// the head count, a drive ID or the checksum that goes with them does
    /// not count.
    pub fn succeeded(&self) -> bool {
        !matches!(self.partition, Some(Err(NotWritten)))
            && self.record.is_none_or(|record| {
                record.written.is_ok() && record.exfat_checksum.is_none_or(|sum| sum.is_ok())
            })
            && self.oem_id.is_none_or(|id| id.is_ok())
            && self
                .disk_signature
                .is_none_or(|signature| signature.is_ok())
    }
}

pub fn apply(device: &mut dyn Device, request: &Request) -> Result<Report, Refusal> {
    if !request.force {
        if let Some(kind) = request.record {
            sanity_check(device, kind).map_err(|reason| Refusal::Record { kind, reason })?;
        }
        if request.oem_id.is_some() {
            identify::ok_to_write_oem_id(device).map_err(Refusal::OemId)?;
        }
        if request.disk_signature.is_some() {
            sanity_check(device, Kind::MbrZero).map_err(Refusal::DiskSignature)?;
        }
    }

    let mut report = Report::default();
    if let Some(heads) = request.partition {
        if request.record.is_none()
            && !request.force
            && !is_fat16_fs(device)
            && !is_fat32_fs(device)
            && !exfat::is_file_system(device)
            && !is_ntfs_fs(device)
        {
            return Err(Refusal::PartitionWithoutFileSystem);
        }
        report.partition = Some(write_partition_info(
            device,
            heads,
            request.record.is_none(),
        ));
    }

    if let Some(kind) = request.record {
        let record = kind.record();
        report.record = Some(RecordReport {
            kind,
            written: written(record.write(device, !request.wipe_label, request.bytes_per_sector)),
            // UPSTREAM-BUG: the exFAT checksum is attempted even when
            // writing the boot record itself failed.
            exfat_checksum: (record.family == Family::Exfat)
                .then(|| written(exfat::write_boot_checksum(device))),
        });
    }

    if let Some(oem_id) = &request.oem_id {
        let mut padded = [b' '; 8];
        for (slot, &byte) in padded.iter_mut().zip(oem_id) {
            *slot = byte;
        }
        report.oem_id = Some(written(device.write_at(identify::OEM_ID_OFFSET, &padded)));
    }

    if let Some(signature) = request.disk_signature {
        report.disk_signature = Some(written(
            device.write_at(identify::DISK_SIGNATURE_OFFSET, &signature.to_le_bytes()),
        ));
    }
    Ok(report)
}

fn write_partition_info(
    device: &mut dyn Device,
    heads: Heads,
    refresh_exfat_checksum: bool,
) -> Result<PartitionReport, NotWritten> {
    let probe = device.probe();
    let fat_or_ntfs =
        |device: &dyn Device| is_fat16_fs(device) || is_fat32_fs(device) || is_ntfs_fs(device);

    // UPSTREAM-BUG: a device that is no partition, a partition holding none
    // of the supported file systems, which forcing allows, and a failed
    // write are not told apart.
    let start_sector = probe.start_sector();
    if start_sector == 0 {
        return Err(NotWritten);
    } else if fat_or_ntfs(device) {
        // The hidden-sectors field holds 32 bits; the rest is dropped.
        written(device.write_at(0x1c, &(start_sector as u32).to_le_bytes()))?;
    } else if exfat::is_file_system(device) {
        written(device.write_at(0x40, &start_sector.to_le_bytes()))?;
    } else {
        return Err(NotWritten);
    }

    let mut report = PartitionReport {
        start_sector,
        heads: None,
        drive_ids: Vec::new(),
        exfat_checksum: None,
    };
    if fat_or_ntfs(device) {
        let wanted = match heads {
            Heads::Kernel => probe.heads(),
            Heads::Given(heads) => heads,
        };
        report.heads = Some(if wanted == 0 {
            Err(NotWritten)
        } else {
            written(device.write_at(0x1a, &wanted.to_le_bytes())).map(|()| wanted)
        });
    }
    for (family, offset) in [
        (Family::Fat16, 0x24),
        (Family::Fat32, 0x40),
        (Family::Exfat, 0x6f),
    ] {
        let present = match family {
            Family::Fat16 => is_fat16_fs(device),
            Family::Fat32 => is_fat32_fs(device),
            _ => exfat::is_file_system(device),
        };
        if present {
            let drive_id = written(device.write_at(offset, &[0x80]));
            report.drive_ids.push((family, drive_id));
            if family == Family::Exfat && refresh_exfat_checksum {
                report.exfat_checksum = Some(written(exfat::write_boot_checksum(device)));
            }
        }
    }
    Ok(report)
}
