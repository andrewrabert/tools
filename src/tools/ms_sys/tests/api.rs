//! What the library writes to a device and what it makes of one, on disk
//! images held in memory.
//!
//! The bytes written are those ms-sys 2.9.0 writes, the C program this one
//! reproduces. Each test spells out the image it expects: the starting
//! image with named payloads and fields placed at their offsets.

use super::super::apply::{
    Heads, NotWritten, PartitionReport, RecordReport, Refusal, Report, Request, apply,
};
use super::super::exfat::boot_checksum;
use super::super::identify::{
    BootRecord, BootSector, Diagnosis, OemId, OemIdUnsuitable, Unsuitable, inspect, smart_select,
};
use super::super::payloads as p;
use super::super::record::{Family, Kind, RECORDS};
use super::harness::{
    Faulty, IMAGE_SIZE, Image, Profile, SIGNATURE, assert_image, assert_untouched, images, patched,
};

/// Bytes and the offset they are expected at.
type Patch = (usize, &'static [u8]);

/// The label a FAT boot record gets when its own is wiped.
const NO_NAME: &[u8] = b"NO NAME    ";

fn nothing() -> Request {
    Request::default()
}

fn write(kind: Kind) -> Request {
    Request {
        record: Some(kind),
        ..nothing()
    }
}

fn forced(request: Request) -> Request {
    Request {
        force: true,
        ..request
    }
}

fn partition(heads: Heads) -> Request {
    Request {
        partition: Some(heads),
        ..nothing()
    }
}

fn oem_id(id: &[u8]) -> Request {
    Request {
        oem_id: Some(id.to_vec()),
        ..nothing()
    }
}

fn disk_signature(signature: u32) -> Request {
    Request {
        disk_signature: Some(signature),
        ..nothing()
    }
}

/// The report of a boot record that went in whole.
fn record_written(kind: Kind) -> Option<RecordReport> {
    Some(RecordReport {
        kind,
        written: Ok(()),
        exfat_checksum: None,
    })
}

fn wrote(kind: Kind) -> Result<Report, Refusal> {
    Ok(Report {
        record: record_written(kind),
        ..Report::default()
    })
}

fn wrote_partition(partition: PartitionReport) -> Result<Report, Refusal> {
    Ok(Report {
        partition: Some(Ok(partition)),
        ..Report::default()
    })
}

fn refused(kind: Kind, reason: Unsuitable) -> Result<Report, Refusal> {
    Err(Refusal::Record { kind, reason })
}

/// The eleven boot sectors of an exFAT volume summed, as the sector after
/// them holds it: the checksum over and over.
fn checksum_sector(image: &[u8], sector: usize) -> Vec<u8> {
    boot_checksum(&image[..11 * sector])
        .to_le_bytes()
        .repeat(sector / 4)
}

/// `image` with the checksum of its boot sectors where exFAT keeps it and
/// its backup.
fn with_checksums(image: &[u8], sector: usize) -> Vec<u8> {
    let sum = checksum_sector(image, sector);
    patched(image, &[(11 * sector, &sum), (23 * sector, &sum)])
}

// Boot records.

/// Where each array of a record goes, as upstream's `write_*` functions
/// place them. A master boot record also gets a boot signature; see
/// [`MBR_KINDS`].
fn layout(kind: Kind) -> &'static [Patch] {
    match kind {
        Kind::MbrDos => &[(0x0, p::MBR_DOS)],
        Kind::Fat12 => &[(0x0, p::BR_FAT12_0X0), (0x3e, p::BR_FAT12_0X3E)],
        Kind::Fat16 => &[(0x0, p::BR_FAT16_0X0), (0x3e, p::BR_FAT16_0X3E)],
        Kind::Fat32 => &[
            (0x0, p::BR_FAT32_0X0),
            (0x52, p::BR_FAT32_0X52),
            (0x3f0, p::BR_FAT32_0X3F0),
        ],
        Kind::Fat32Nt5 => &[
            (0x0, p::BR_FAT32_0X0),
            (0x52, p::BR_FAT32NT5_0X52),
            (0x3f0, p::BR_FAT32NT5_0X3F0),
            (0x1800, p::BR_FAT32NT5_0X1800),
        ],
        Kind::Fat32Nt6 => &[
            (0x0, p::BR_FAT32NT6_0X0),
            (0x52, p::BR_FAT32NT6_0X52),
            (0x5fe, SIGNATURE),
            (0x1800, p::BR_FAT32NT6_0X1800),
        ],
        Kind::ExfatNt6 => unreachable!("placed by sector size; see the exFAT tests"),
        Kind::Mbr95b => &[(0x0, p::MBR_95B_0X0), (0xe0, p::MBR_95B_0X0E0)],
        Kind::Mbr2000 => &[(0x0, p::MBR_2000)],
        Kind::MbrVista => &[(0x0, p::MBR_VISTA)],
        Kind::MbrWin7 => &[(0x0, p::MBR_WIN7)],
        Kind::MbrSyslinux => &[(0x0, p::MBR_SYSLINUX)],
        // Everything up to the partition table, disk signature included.
        Kind::MbrZero => &[(0x0, &[0; 0x1be])],
        Kind::Fat16Fd => &[(0x0, p::BR_FAT16_0X0), (0x3e, p::BR_FAT16FD_0X3E)],
        Kind::Fat32Fd => &[
            (0x0, p::BR_FAT32_0X0),
            (0x52, p::BR_FAT32FD_0X52),
            (0x3f0, p::BR_FAT32FD_0X3F0),
        ],
        Kind::Fat32Pe => &[
            (0x0, p::BR_FAT32_0X0),
            (0x52, p::BR_FAT32PE_0X52),
            (0x3f0, p::BR_FAT32PE_0X3F0),
            (0x1800, p::BR_FAT32PE_0X1800),
        ],
        Kind::Ntfs => &[(0x0, p::BR_NTFS_0X0), (0x54, p::BR_NTFS_0X54)],
        Kind::MbrGptSyslinux => &[(0x0, p::MBR_GPT_SYSLINUX)],
        Kind::MbrRufus => &[(0x0, p::MBR_RUFUS)],
        Kind::MbrReactos => &[(0x0, p::MBR_REACTOS)],
        Kind::Fat16Ros => &[(0x0, p::BR_FAT16ROS_0X0), (0x3e, p::BR_FAT16ROS_0X3E)],
        Kind::Fat32Ros => &[
            (0x0, p::BR_FAT32_0X0),
            (0x52, p::BR_FAT32ROS_0X52),
            (0x3f0, p::BR_FAT32ROS_0X3F0),
            (0x1c00, p::BR_FAT32ROS_0X1C00),
        ],
        Kind::Fat32Kos => &[(0x0, p::BR_FAT32_0X0), (0x52, p::BR_FAT32KOS_0X52)],
        Kind::MbrGrub4dos => &[(0x0, p::MBR_GRUB)],
        Kind::MbrGrub2 => &[(0x0, p::MBR_GRUB2)],
        Kind::MbrKolibrios => &[(0x0, p::MBR_KOLIBRI)],
    }
}

const MBR_KINDS: [Kind; 13] = [
    Kind::MbrDos,
    Kind::Mbr95b,
    Kind::Mbr2000,
    Kind::MbrVista,
    Kind::MbrWin7,
    Kind::MbrSyslinux,
    Kind::MbrZero,
    Kind::MbrGptSyslinux,
    Kind::MbrRufus,
    Kind::MbrReactos,
    Kind::MbrGrub4dos,
    Kind::MbrGrub2,
    Kind::MbrKolibrios,
];

/// `image` after `kind` is written to it: its arrays in place, and a boot
/// signature if it is a master boot record.
fn with_record(image: &[u8], kind: Kind) -> Vec<u8> {
    let written = patched(image, layout(kind));
    if MBR_KINDS.contains(&kind) {
        patched(&written, &[(0x1fe, SIGNATURE)])
    } else {
        written
    }
}

/// Every record but exFAT's, forced onto every image: each array of the
/// record is at its offset, a master boot record is signed, and nothing
/// else has changed. An image too short for the record grows to hold it.
#[test]
fn every_record_writes_its_arrays_and_nothing_else() {
    for image in &images() {
        for kind in RECORDS.iter().map(|record| record.kind) {
            if kind == Kind::ExfatNt6 {
                continue;
            }
            let context = (&image.name, kind);
            let mut device = image.device(Profile::FILE);
            assert_eq!(
                apply(&mut device, &forced(write(kind))),
                wrote(kind),
                "{context:?}"
            );
            assert_image(&device.bytes, &with_record(&image.bytes, kind), context);
        }
    }
}

/// A boot record goes in around the parameters the file system keeps in
/// its boot sector: a jump and an OEM ID before them, code after.
#[test]
fn a_boot_record_keeps_the_file_system_parameters() {
    use Kind::*;
    for (fixture, code, kinds) in [
        ("fat12", 0x3e, &[Fat12][..]),
        ("fat16", 0x3e, &[Fat16, Fat16Fd, Fat16Ros]),
        (
            "fat32",
            0x52,
            &[
                Fat32, Fat32Nt5, Fat32Nt6, Fat32Pe, Fat32Fd, Fat32Ros, Fat32Kos,
            ],
        ),
        ("ntfs", 0x54, &[Ntfs]),
    ] {
        let image = Image::fixture(fixture);
        for &kind in kinds {
            let mut device = image.device(Profile::PARTITION);
            assert_eq!(
                apply(&mut device, &forced(write(kind))),
                wrote(kind),
                "{kind:?}"
            );
            assert_eq!(device.bytes[0xb..code], image.bytes[0xb..code], "{kind:?}");
        }
    }
}

/// A master boot record leaves the partition table alone, and all but the
/// zeroed one leave the disk signature in front of it.
#[test]
fn a_master_boot_record_keeps_the_partition_table() {
    let image = Image::zeros()
        .with("code", 0x0, &[0xee; 0x1b8])
        .with(
            "disk-signature",
            0x1b8,
            &[0x78, 0x56, 0x34, 0x12, 0x5a, 0x5a],
        )
        .with("partition-table", 0x1be, &[0xdd; 0x40]);
    for kind in MBR_KINDS {
        let mut device = image.device(Profile::DISK);
        assert_eq!(apply(&mut device, &write(kind)), wrote(kind), "{kind:?}");
        assert_ne!(device.bytes[..0x1b8], image.bytes[..0x1b8], "{kind:?}");
        let kept = if kind == Kind::MbrZero { 0x1be } else { 0x1b8 };
        assert_eq!(
            device.bytes[kept..0x1fe],
            image.bytes[kept..0x1fe],
            "{kind:?}"
        );
        assert_eq!(&device.bytes[0x1fe..0x200], SIGNATURE, "{kind:?}");
        assert_eq!(device.bytes[0x200..], image.bytes[0x200..], "{kind:?}");
    }
    let mut device = image.device(Profile::DISK);
    apply(&mut device, &write(Kind::MbrZero)).unwrap();
    assert_eq!(device.bytes[..0x1be], [0; 0x1be]);
}

/// Wiping the label puts `NO NAME` where the file system keeps it: at 0x2b
/// for FAT12 and FAT16, at 0x47 for FAT32. Other records have no label to
/// wipe and are written as usual.
#[test]
fn wiping_the_label() {
    use Kind::*;
    let wiped = |kind| Request {
        wipe_label: true,
        ..forced(write(kind))
    };
    for (fixture, label, offset, kinds) in [
        ("fat12", "FIXTURE12  ", 0x2b, &[Fat12][..]),
        ("fat16", "FIXTURE16  ", 0x2b, &[Fat16, Fat16Fd, Fat16Ros]),
        (
            "fat32",
            "FIXTURE32  ",
            0x47,
            &[
                Fat32, Fat32Nt5, Fat32Nt6, Fat32Pe, Fat32Fd, Fat32Ros, Fat32Kos,
            ],
        ),
    ] {
        let image = Image::fixture(fixture);
        assert_eq!(&image.bytes[offset..offset + 11], label.as_bytes());
        for &kind in kinds {
            let mut kept = image.device(Profile::FILE);
            assert_eq!(apply(&mut kept, &forced(write(kind))), wrote(kind));
            assert_eq!(&kept.bytes[offset..offset + 11], label.as_bytes());

            let mut device = image.device(Profile::FILE);
            assert_eq!(apply(&mut device, &wiped(kind)), wrote(kind));
            let expected = patched(&kept.bytes, &[(offset, NO_NAME)]);
            assert_image(&device.bytes, &expected, kind);
        }
    }

    for (fixture, kind) in [("ntfs", Ntfs), ("ntfs", Mbr2000), ("fat16", MbrZero)] {
        let image = Image::fixture(fixture);
        let mut kept = image.device(Profile::FILE);
        apply(&mut kept, &forced(write(kind))).unwrap();
        let mut device = image.device(Profile::FILE);
        assert_eq!(apply(&mut device, &wiped(kind)), wrote(kind));
        assert_image(&device.bytes, &kept.bytes, kind);
    }
}

/// With sectors larger than 512 bytes, a master boot record is signed
/// every 512 bytes up to the end of its sector. No other record heeds the
/// sector size it is given.
#[test]
fn a_master_boot_record_is_signed_throughout_its_sector() {
    let with_sectors = |bytes_per_sector, kind| Request {
        bytes_per_sector,
        ..forced(write(kind))
    };
    for (bytes_per_sector, signatures) in [(512, 1), (513, 1), (1024, 2), (4096, 8), (65536, 128)] {
        for image in [Image::zeros(), Image::new("empty", Vec::new())] {
            let mut device = image.device(Profile::FILE);
            assert_eq!(
                apply(&mut device, &with_sectors(bytes_per_sector, Kind::MbrDos)),
                wrote(Kind::MbrDos)
            );
            let mut payloads = vec![(0x0, p::MBR_DOS)];
            payloads.extend((0..signatures).map(|sector| (sector * 0x200 + 0x1fe, SIGNATURE)));
            assert_image(
                &device.bytes,
                &patched(&image.bytes, &payloads),
                (bytes_per_sector, &image.name),
            );
        }
    }

    for kind in [Kind::Fat32, Kind::Ntfs] {
        let image = Image::zeros();
        let mut usual = image.device(Profile::FILE);
        apply(&mut usual, &forced(write(kind))).unwrap();
        let mut device = image.device(Profile::FILE);
        assert_eq!(apply(&mut device, &with_sectors(4096, kind)), wrote(kind));
        assert_image(&device.bytes, &usual.bytes, kind);
    }
}

/// The exFAT record is boot code at 0x78 of the boot sector and of its
/// backup twelve sectors on, followed by a fresh checksum of each. The
/// sector size is the device's.
#[test]
fn the_exfat_record_goes_to_the_boot_sector_and_its_backup() {
    let image = Image::fixture("exfat");
    let written = Ok(Report {
        record: Some(RecordReport {
            kind: Kind::ExfatNt6,
            written: Ok(()),
            exfat_checksum: Some(Ok(())),
        }),
        ..Report::default()
    });
    for (profile, sector) in [
        (Profile::PARTITION, 512),
        (Profile::FILE, 512),
        (Profile::PARTITION_4K, 4096),
    ] {
        let mut device = image.device(profile);
        let request = Request {
            force: profile == Profile::FILE,
            ..write(Kind::ExfatNt6)
        };
        assert_eq!(apply(&mut device, &request), written, "{}", profile.name);
        let code = patched(
            &image.bytes,
            &[
                (0x78, p::BR_EXFATNT6_0X78),
                (12 * sector + 0x78, p::BR_EXFATNT6_0X78),
            ],
        );
        assert_image(&device.bytes, &with_checksums(&code, sector), profile.name);
    }
}

/// Sector sizes no disk has. With no room in a sector for a checksum none
/// is written, which counts as success; with sectors of no size at all the
/// backup lands on the boot sector and the checksum fails.
#[test]
fn the_exfat_record_on_absurd_sectors() {
    let image = Image::fixture("exfat");
    let report = |checksum| {
        Ok(Report {
            record: Some(RecordReport {
                kind: Kind::ExfatNt6,
                written: Ok(()),
                exfat_checksum: Some(checksum),
            }),
            ..Report::default()
        })
    };

    let mut device = image.device(Profile::PARTITION_2_BYTE_SECTORS);
    assert_eq!(apply(&mut device, &write(Kind::ExfatNt6)), report(Ok(())));
    let expected = patched(
        &image.bytes,
        &[
            (0x78, p::BR_EXFATNT6_0X78),
            (12 * 2 + 0x78, p::BR_EXFATNT6_0X78),
        ],
    );
    assert_image(&device.bytes, &expected, "2-byte sectors");

    let mut device = image.device(Profile::PARTITION_0_BYTE_SECTORS);
    let result = apply(&mut device, &write(Kind::ExfatNt6));
    assert_eq!(result, report(Err(NotWritten)));
    assert!(!result.unwrap().succeeded());
    let expected = patched(&image.bytes, &[(0x78, p::BR_EXFATNT6_0X78)]);
    assert_image(&device.bytes, &expected, "0-byte sectors");
}

/// The checksum this program computes is the one `mkfs.exfat` left in the
/// fixture.
#[test]
fn the_exfat_checksum_agrees_with_mkfs() {
    let image = Image::fixture("exfat");
    let sum = checksum_sector(&image.bytes, 512);
    assert_eq!(image.bytes[11 * 512..12 * 512], sum);
    assert_eq!(image.bytes[23 * 512..24 * 512], sum);
}

// Partition information.

/// On FAT and NTFS: the start sector in the hidden sectors field at 0x1c,
/// the head count at 0x1a, and drive ID 0x80 at 0x24 for FAT16 and at 0x40
/// for FAT32. NTFS has no drive ID; FAT12 is not served at all.
#[test]
fn partition_information_on_fat_and_ntfs() {
    let both = Image::fixture("fat16").with("fat32", 0x52, b"FAT32   ");
    let cases: [(Image, &[(Family, usize)]); 4] = [
        (Image::fixture("fat16"), &[(Family::Fat16, 0x24)]),
        (Image::fixture("fat32"), &[(Family::Fat32, 0x40)]),
        (Image::fixture("ntfs"), &[]),
        (both, &[(Family::Fat16, 0x24), (Family::Fat32, 0x40)]),
    ];
    for (image, drive_ids) in cases {
        let mut device = image.device(Profile::PARTITION);
        assert_eq!(
            apply(&mut device, &partition(Heads::Kernel)),
            wrote_partition(PartitionReport {
                start_sector: 2048,
                heads: Some(Ok(255)),
                drive_ids: drive_ids
                    .iter()
                    .map(|&(family, _)| (family, Ok(())))
                    .collect(),
                exfat_checksum: None,
            }),
            "{}",
            image.name
        );
        let mut fields: Vec<(usize, &[u8])> =
            vec![(0x1c, &[0x00, 0x08, 0x00, 0x00]), (0x1a, &[0xff, 0x00])];
        fields.extend(drive_ids.iter().map(|&(_, offset)| (offset, &[0x80][..])));
        assert_image(&device.bytes, &patched(&image.bytes, &fields), &image.name);
    }

    let fat12 = Image::fixture("fat12");
    let mut device = fat12.device(Profile::PARTITION);
    assert_eq!(
        apply(&mut device, &partition(Heads::Kernel)),
        Err(Refusal::PartitionWithoutFileSystem)
    );
    assert_eq!(
        apply(&mut device, &forced(partition(Heads::Kernel))),
        Ok(Report {
            partition: Some(Err(NotWritten)),
            ..Report::default()
        })
    );
    assert_untouched(&device, &fat12, "fat12");
}

/// The head count is the kernel's unless one is given. A count of zero,
/// given or the kernel's, is not written, and the rest still is.
#[test]
fn the_head_count_written_with_partition_information() {
    let image = Image::fixture("fat16");
    let cases: [(Profile, Heads, Option<u16>); 7] = [
        (Profile::PARTITION, Heads::Kernel, Some(255)),
        (Profile::PARTITION, Heads::Given(16), Some(16)),
        (Profile::PARTITION, Heads::Given(1), Some(1)),
        (Profile::PARTITION, Heads::Given(65535), Some(65535)),
        (Profile::PARTITION, Heads::Given(0), None),
        (Profile::PARTITION_0_HEADS, Heads::Kernel, None),
        (Profile::PARTITION_0_HEADS, Heads::Given(4), Some(4)),
    ];
    for (profile, heads, written) in cases {
        let context = (profile.name, heads);
        let mut device = image.device(profile);
        assert_eq!(
            apply(&mut device, &partition(heads)),
            wrote_partition(PartitionReport {
                start_sector: 2048,
                heads: Some(written.ok_or(NotWritten)),
                drive_ids: vec![(Family::Fat16, Ok(()))],
                exfat_checksum: None,
            }),
            "{context:?}"
        );
        let count = written.map(u16::to_le_bytes);
        let mut fields: Vec<(usize, &[u8])> =
            vec![(0x1c, &[0x00, 0x08, 0x00, 0x00]), (0x24, &[0x80])];
        if let Some(count) = &count {
            fields.push((0x1a, count));
        }
        assert_image(&device.bytes, &patched(&image.bytes, &fields), context);
    }
}

/// The start sector and head count are whatever the device reports. The
/// hidden sectors field of FAT and NTFS holds 32 bits, and a start sector
/// beyond that loses the rest.
#[test]
fn the_start_sector_written_with_partition_information() {
    let image = Image::fixture("fat32");
    let cases: [(Profile, u64, [u8; 4], u8); 4] = [
        (Profile::PARTITION_16_HEADS, 63, [63, 0, 0, 0], 16),
        (Profile::GEOMETRY_ONLY, 2048, [0x00, 0x08, 0, 0], 255),
        (Profile::FLOPPY_PARTITION, 2048, [0x00, 0x08, 0, 0], 2),
        (
            Profile::PARTITION_BEYOND_32_BITS,
            0x1_2345_6789,
            [0x89, 0x67, 0x45, 0x23],
            255,
        ),
    ];
    for (profile, start_sector, hidden, heads) in cases {
        let mut device = image.device(profile);
        assert_eq!(
            apply(&mut device, &partition(Heads::Kernel)),
            wrote_partition(PartitionReport {
                start_sector,
                heads: Some(Ok(u16::from(heads))),
                drive_ids: vec![(Family::Fat32, Ok(()))],
                exfat_checksum: None,
            }),
            "{}",
            profile.name
        );
        let expected = patched(
            &image.bytes,
            &[(0x1c, &hidden), (0x1a, &[heads, 0]), (0x40, &[0x80])],
        );
        assert_image(&device.bytes, &expected, profile.name);
    }
}

/// A device that does not start somewhere on a disk has no partition
/// information to write. That is a failed write and not a refusal.
#[test]
fn partition_information_needs_a_partition() {
    for fixture in ["fat16", "fat32", "ntfs", "exfat"] {
        let image = Image::fixture(fixture);
        for profile in [
            Profile::FILE,
            Profile::DISK,
            Profile::FLOPPY,
            Profile::FLOPPY_IOCTL_ONLY,
            Profile::SIZE_ONLY,
        ] {
            let context = (fixture, profile.name);
            let mut device = image.device(profile);
            let result = apply(&mut device, &partition(Heads::Kernel));
            assert_eq!(
                result,
                Ok(Report {
                    partition: Some(Err(NotWritten)),
                    ..Report::default()
                }),
                "{context:?}"
            );
            assert!(!result.unwrap().succeeded());
            assert_untouched(&device, &image, context);
        }
    }
}

/// Without a file system it knows, the program refuses; forced, it tries
/// and fails. A record written in the same call counts as a file system.
#[test]
fn partition_information_needs_a_file_system() {
    for image in [Image::zeros(), Image::signed(), Image::fixture("fat12")] {
        let mut device = image.device(Profile::PARTITION);
        assert_eq!(
            apply(&mut device, &partition(Heads::Kernel)),
            Err(Refusal::PartitionWithoutFileSystem),
            "{}",
            image.name
        );
        assert_eq!(
            apply(&mut device, &forced(partition(Heads::Given(4)))),
            Ok(Report {
                partition: Some(Err(NotWritten)),
                ..Report::default()
            }),
            "{}",
            image.name
        );
        assert_untouched(&device, &image, &image.name);
    }

    // The partition information comes first and goes by what is on the
    // device then, so here it fails although FAT32 follows.
    let image = Image::zeros();
    let mut device = image.device(Profile::PARTITION);
    assert_eq!(
        apply(
            &mut device,
            &Request {
                partition: Some(Heads::Kernel),
                ..forced(write(Kind::Fat32))
            }
        ),
        Ok(Report {
            partition: Some(Err(NotWritten)),
            record: record_written(Kind::Fat32),
            ..Report::default()
        })
    );
    let mut alone = image.device(Profile::PARTITION);
    apply(&mut alone, &forced(write(Kind::Fat32))).unwrap();
    assert_image(&device.bytes, &alone.bytes, "fat32 on zeros");
}

/// On exFAT: the start sector, all 64 bits of it, at 0x40, drive ID 0x80
/// at 0x6f, no head count, and a fresh checksum of the boot sectors.
#[test]
fn partition_information_on_exfat() {
    let image = Image::fixture("exfat");
    let written = |start_sector, checksum| {
        wrote_partition(PartitionReport {
            start_sector,
            heads: None,
            drive_ids: vec![(Family::Exfat, Ok(()))],
            exfat_checksum: Some(checksum),
        })
    };
    let fields = |start_sector: u64| {
        patched(
            &image.bytes,
            &[(0x40, &start_sector.to_le_bytes()), (0x6f, &[0x80])],
        )
    };

    for (profile, start_sector, sector) in [
        (Profile::PARTITION, 2048, 512),
        (Profile::PARTITION_16_HEADS, 63, 512),
        (Profile::PARTITION_BEYOND_32_BITS, 0x1_2345_6789, 512),
        (Profile::PARTITION_4K, 2048, 4096),
    ] {
        let mut device = image.device(profile);
        assert_eq!(
            apply(&mut device, &partition(Heads::Given(16))),
            written(start_sector, Ok(())),
            "{}",
            profile.name
        );
        assert_image(
            &device.bytes,
            &with_checksums(&fields(start_sector), sector),
            profile.name,
        );
    }

    let mut device = image.device(Profile::PARTITION_2_BYTE_SECTORS);
    assert_eq!(
        apply(&mut device, &partition(Heads::Kernel)),
        written(2048, Ok(()))
    );
    assert_image(&device.bytes, &fields(2048), "2-byte sectors");

    let mut device = image.device(Profile::PARTITION_0_BYTE_SECTORS);
    let result = apply(&mut device, &partition(Heads::Kernel));
    assert_eq!(result, written(2048, Err(NotWritten)));
    // UPSTREAM-BUG: once the start sector is in, nothing else counts.
    assert!(result.unwrap().succeeded());
    assert_image(&device.bytes, &fields(2048), "0-byte sectors");
}

/// With a record in the same call the partition information goes in first,
/// by what the device held before the record, and the record is written
/// over it. The exFAT checksum is then left to the record.
#[test]
fn partition_information_with_a_record() {
    let fat16 = Image::fixture("fat16");
    let mut device = fat16.device(Profile::PARTITION);
    let request = Request {
        partition: Some(Heads::Kernel),
        ..write(Kind::Fat16)
    };
    assert_eq!(
        apply(&mut device, &request),
        Ok(Report {
            partition: Some(Ok(PartitionReport {
                start_sector: 2048,
                heads: Some(Ok(255)),
                drive_ids: vec![(Family::Fat16, Ok(()))],
                exfat_checksum: None,
            })),
            record: record_written(Kind::Fat16),
            ..Report::default()
        })
    );
    let expected = patched(
        &fat16.bytes,
        &[
            (0x1c, &[0x00, 0x08, 0x00, 0x00]),
            (0x1a, &[0xff, 0x00]),
            (0x24, &[0x80]),
            (0x0, p::BR_FAT16_0X0),
            (0x3e, p::BR_FAT16_0X3E),
        ],
    );
    assert_image(&device.bytes, &expected, "fat16");

    // NTFS before, FAT32 after: no drive ID, though FAT32 has one.
    let ntfs = Image::fixture("ntfs");
    let mut device = ntfs.device(Profile::PARTITION);
    let request = Request {
        partition: Some(Heads::Given(16)),
        ..forced(write(Kind::Fat32))
    };
    assert_eq!(
        apply(&mut device, &request),
        Ok(Report {
            partition: Some(Ok(PartitionReport {
                start_sector: 2048,
                heads: Some(Ok(16)),
                drive_ids: vec![],
                exfat_checksum: None,
            })),
            record: record_written(Kind::Fat32),
            ..Report::default()
        })
    );
    let expected = patched(
        &ntfs.bytes,
        &[
            (0x1c, &[0x00, 0x08, 0x00, 0x00]),
            (0x1a, &[0x10, 0x00]),
            (0x0, p::BR_FAT32_0X0),
            (0x52, p::BR_FAT32_0X52),
            (0x3f0, p::BR_FAT32_0X3F0),
        ],
    );
    assert_image(&device.bytes, &expected, "ntfs");

    let exfat = Image::fixture("exfat");
    let mut device = exfat.device(Profile::PARTITION);
    let request = Request {
        partition: Some(Heads::Kernel),
        ..write(Kind::ExfatNt6)
    };
    assert_eq!(
        apply(&mut device, &request),
        Ok(Report {
            partition: Some(Ok(PartitionReport {
                start_sector: 2048,
                heads: None,
                drive_ids: vec![(Family::Exfat, Ok(()))],
                exfat_checksum: None,
            })),
            record: Some(RecordReport {
                kind: Kind::ExfatNt6,
                written: Ok(()),
                exfat_checksum: Some(Ok(())),
            }),
            ..Report::default()
        })
    );
    let expected = patched(
        &exfat.bytes,
        &[
            (0x40, &2048u64.to_le_bytes()),
            (0x6f, &[0x80]),
            (0x78, p::BR_EXFATNT6_0X78),
            (12 * 512 + 0x78, p::BR_EXFATNT6_0X78),
        ],
    );
    assert_image(&device.bytes, &with_checksums(&expected, 512), "exfat");
}

// OEM ID and disk signature.

/// The OEM ID is the eight bytes at 0x3: the ID given, whatever bytes it
/// is made of, cut to eight or padded with spaces.
#[test]
fn the_oem_id_is_cut_or_padded_to_eight_bytes() {
    let image = Image::fixture("fat16");
    let cases: [(&[u8], &[u8; 8]); 7] = [
        (b"MSWIN4.1", b"MSWIN4.1"),
        (b"X", b"X       "),
        (b"", b"        "),
        (b"LONGER THAN EIGHT", b"LONGER T"),
        (b"--help", b"--help  "),
        (b"\xff\xfe\x80oem", b"\xff\xfe\x80oem  "),
        (b"a\0b", b"a\0b     "),
    ];
    for (given, held) in cases {
        let mut device = image.device(Profile::PARTITION);
        assert_eq!(
            apply(&mut device, &oem_id(given)),
            Ok(Report {
                oem_id: Some(Ok(())),
                ..Report::default()
            }),
            "{:?}",
            given.escape_ascii().to_string()
        );
        assert_image(
            &device.bytes,
            &patched(&image.bytes, &[(0x3, held)]),
            given.escape_ascii().to_string(),
        );
    }
}

/// An OEM ID is for FAT on the device its record is for: FAT12 on a
/// floppy, FAT16 and FAT32 on a partition.
///
/// UPSTREAM-BUG: NTFS and exFAT keep an OEM ID in the same place and are
/// turned away.
#[test]
fn an_oem_id_wants_fat_on_the_right_device() {
    let cases = [
        ("fat12", Profile::FLOPPY, Ok(())),
        ("fat12", Profile::FLOPPY_IOCTL_ONLY, Ok(())),
        ("fat16", Profile::PARTITION, Ok(())),
        ("fat32", Profile::PARTITION, Ok(())),
        ("fat32", Profile::GEOMETRY_ONLY, Ok(())),
        (
            "fat12",
            Profile::PARTITION,
            Err(OemIdUnsuitable::WrongDevice),
        ),
        ("fat16", Profile::FLOPPY, Err(OemIdUnsuitable::WrongDevice)),
        ("fat16", Profile::FILE, Err(OemIdUnsuitable::WrongDevice)),
        ("fat32", Profile::DISK, Err(OemIdUnsuitable::WrongDevice)),
        (
            "ntfs",
            Profile::PARTITION,
            Err(OemIdUnsuitable::NoFatFileSystem),
        ),
        (
            "exfat",
            Profile::PARTITION,
            Err(OemIdUnsuitable::NoFatFileSystem),
        ),
        ("ntfs", Profile::FILE, Err(OemIdUnsuitable::NoFatFileSystem)),
    ];
    for (fixture, profile, verdict) in cases {
        let context = (fixture, profile.name);
        let image = Image::fixture(fixture);
        let mut device = image.device(profile);
        let result = apply(&mut device, &oem_id(b"NEWOEM"));
        match verdict {
            Ok(()) => {
                assert!(result.is_ok(), "{context:?}");
                assert_eq!(&device.bytes[0x3..0xb], b"NEWOEM  ", "{context:?}");
            }
            Err(reason) => {
                assert_eq!(result, Err(Refusal::OemId(reason)), "{context:?}");
                assert_untouched(&device, &image, context);
                // Forced, it is written all the same.
                assert!(apply(&mut device, &forced(oem_id(b"NEWOEM"))).is_ok());
                assert_image(
                    &device.bytes,
                    &patched(&image.bytes, &[(0x3, b"NEWOEM  ")]),
                    context,
                );
            }
        }
    }
}

/// The Windows disk signature is four bytes at 0x1b8, least significant
/// first.
#[test]
fn the_disk_signature_is_four_bytes_before_the_partition_table() {
    let image = Image::signed().with("code", 0x0, &[0xee; 0x1be]);
    let cases: [(u32, [u8; 4]); 5] = [
        (0x1234_abcd, [0xcd, 0xab, 0x34, 0x12]),
        (0xdead_beef, [0xef, 0xbe, 0xad, 0xde]),
        (0, [0, 0, 0, 0]),
        (0x7f, [0x7f, 0, 0, 0]),
        (0xffff_ffff, [0xff, 0xff, 0xff, 0xff]),
    ];
    for (signature, held) in cases {
        let mut device = image.device(Profile::DISK);
        assert_eq!(
            apply(&mut device, &disk_signature(signature)),
            Ok(Report {
                disk_signature: Some(Ok(())),
                ..Report::default()
            }),
            "{signature:#x}"
        );
        assert_image(
            &device.bytes,
            &patched(&image.bytes, &[(0x1b8, &held)]),
            signature,
        );
    }
}

/// A disk signature goes where a master boot record may go: on a whole
/// hard disk.
#[test]
fn a_disk_signature_wants_a_whole_disk() {
    let image = Image::signed();
    for (profile, reason) in [
        (Profile::FILE, Unsuitable::NotDiskDevice),
        (Profile::FLOPPY, Unsuitable::Floppy),
        (Profile::PARTITION, Unsuitable::Partition),
    ] {
        let mut device = image.device(profile);
        assert_eq!(
            apply(&mut device, &disk_signature(0xcafe)),
            Err(Refusal::DiskSignature(reason)),
            "{}",
            profile.name
        );
        assert_untouched(&device, &image, profile.name);
        assert!(apply(&mut device, &forced(disk_signature(0xcafe))).is_ok());
        assert_eq!(device.bytes[0x1b8..0x1bc], [0xfe, 0xca, 0, 0]);
    }
}

/// Everything one call can write, in the order it is written: partition
/// information, the record with the label wiped, then the OEM ID and the
/// disk signature over the record.
#[test]
fn everything_at_once() {
    let image = Image::fixture("fat16");
    let mut device = image.device(Profile::PARTITION);
    let request = Request {
        record: Some(Kind::Fat16),
        force: true,
        wipe_label: true,
        partition: Some(Heads::Given(8)),
        bytes_per_sector: 2048,
        oem_id: Some(b"BOTH".to_vec()),
        disk_signature: Some(0xcafe),
    };
    assert_eq!(
        apply(&mut device, &request),
        Ok(Report {
            partition: Some(Ok(PartitionReport {
                start_sector: 2048,
                heads: Some(Ok(8)),
                drive_ids: vec![(Family::Fat16, Ok(()))],
                exfat_checksum: None,
            })),
            record: record_written(Kind::Fat16),
            oem_id: Some(Ok(())),
            disk_signature: Some(Ok(())),
        })
    );
    let expected = patched(
        &image.bytes,
        &[
            (0x1c, &[0x00, 0x08, 0x00, 0x00]),
            (0x1a, &[0x08, 0x00]),
            (0x24, &[0x80]),
            (0x0, p::BR_FAT16_0X0),
            (0x2b, NO_NAME),
            (0x3e, p::BR_FAT16_0X3E),
            (0x3, b"BOTH    "),
            (0x1b8, &[0xfe, 0xca, 0x00, 0x00]),
        ],
    );
    assert_image(&device.bytes, &expected, "fat16");
}

// What is refused.

/// Unforced, a record goes only where it belongs: a master boot record on
/// a whole hard disk, FAT12 on a floppy, the rest on a partition, each on
/// its own file system.
#[test]
fn a_record_wants_its_own_device_and_file_system() {
    use Unsuitable::*;
    let cases = [
        ("ntfs", Kind::Mbr2000, Profile::DISK, Ok(())),
        ("zeros", Kind::MbrZero, Profile::SIZE_ONLY, Ok(())),
        ("ntfs", Kind::Mbr2000, Profile::FILE, Err(NotDiskDevice)),
        (
            "ntfs",
            Kind::Mbr2000,
            Profile::FLOPPY_IOCTL_ONLY,
            Err(NotDiskDevice),
        ),
        ("ntfs", Kind::Mbr2000, Profile::FLOPPY, Err(Floppy)),
        (
            "ntfs",
            Kind::Mbr2000,
            Profile::FLOPPY_PARTITION,
            Err(Floppy),
        ),
        ("ntfs", Kind::Mbr2000, Profile::PARTITION, Err(Partition)),
        (
            "ntfs",
            Kind::Mbr2000,
            Profile::GEOMETRY_ONLY,
            Err(Partition),
        ),
        ("fat12", Kind::Fat12, Profile::FLOPPY, Ok(())),
        ("fat12", Kind::Fat12, Profile::FLOPPY_IOCTL_ONLY, Ok(())),
        ("fat12", Kind::Fat12, Profile::FLOPPY_PARTITION, Ok(())),
        ("fat12", Kind::Fat12, Profile::FILE, Err(NotFloppy)),
        ("fat12", Kind::Fat12, Profile::PARTITION, Err(NotFloppy)),
        ("fat16", Kind::Fat12, Profile::FLOPPY, Err(NoFileSystem)),
        ("fat16", Kind::Fat16, Profile::PARTITION, Ok(())),
        ("fat16", Kind::Fat16Fd, Profile::GEOMETRY_ONLY, Ok(())),
        ("fat16", Kind::Fat16Ros, Profile::FLOPPY_PARTITION, Ok(())),
        ("fat16", Kind::Fat16, Profile::FILE, Err(NotPartition)),
        ("fat16", Kind::Fat16, Profile::DISK, Err(NotPartition)),
        ("fat16", Kind::Fat16, Profile::FLOPPY, Err(NotPartition)),
        ("fat16", Kind::Fat16, Profile::SIZE_ONLY, Err(NotPartition)),
        ("fat12", Kind::Fat16, Profile::PARTITION, Err(NoFileSystem)),
        ("fat32", Kind::Fat16, Profile::PARTITION, Err(NoFileSystem)),
        ("fat32", Kind::Fat32, Profile::PARTITION, Ok(())),
        ("fat32", Kind::Fat32Nt6, Profile::PARTITION_4K, Ok(())),
        ("fat32", Kind::Fat32Kos, Profile::DISK, Err(NotPartition)),
        (
            "fat16",
            Kind::Fat32Pe,
            Profile::PARTITION,
            Err(NoFileSystem),
        ),
        ("ntfs", Kind::Ntfs, Profile::PARTITION, Ok(())),
        ("ntfs", Kind::Ntfs, Profile::FILE, Err(NotPartition)),
        ("exfat", Kind::Ntfs, Profile::PARTITION, Err(NoFileSystem)),
        ("exfat", Kind::ExfatNt6, Profile::PARTITION, Ok(())),
        ("exfat", Kind::ExfatNt6, Profile::DISK, Err(NotPartition)),
        (
            "ntfs",
            Kind::ExfatNt6,
            Profile::PARTITION,
            Err(NoFileSystem),
        ),
        (
            "zeros",
            Kind::ExfatNt6,
            Profile::PARTITION,
            Err(NoFileSystem),
        ),
    ];
    for (name, kind, profile, verdict) in cases {
        let context = (name, kind, profile.name);
        let image = super::harness::image(name);
        let mut device = image.device(profile);
        let result = apply(&mut device, &write(kind));
        match verdict {
            Ok(()) => {
                let report = result.unwrap_or_else(|refusal| panic!("{context:?}: {refusal:?}"));
                assert!(report.succeeded(), "{context:?}");
                assert_ne!(device.bytes, image.bytes, "{context:?}");
            }
            Err(reason) => {
                assert_eq!(result, refused(kind, reason), "{context:?}");
                assert_untouched(&device, &image, context);
            }
        }
    }
}

/// Whatever is asked of whatever device, a refusal leaves the image as it
/// was, and forcing the same request is never refused.
#[test]
fn a_refusal_writes_nothing_and_force_is_never_refused() {
    let mut requests: Vec<Request> = RECORDS.iter().map(|record| write(record.kind)).collect();
    requests.extend([
        partition(Heads::Kernel),
        partition(Heads::Given(16)),
        oem_id(b"NEWOEM"),
        disk_signature(0xcafe),
        Request {
            partition: Some(Heads::Kernel),
            oem_id: Some(b"NEWOEM".to_vec()),
            disk_signature: Some(0xcafe),
            wipe_label: true,
            ..write(Kind::Fat32)
        },
    ]);
    let mut refusals = 0;
    for image in &images() {
        for profile in Profile::all() {
            for request in &requests {
                let context = (&image.name, profile.name, request);
                let mut device = image.device(profile);
                if apply(&mut device, request).is_err() {
                    refusals += 1;
                    assert_untouched(&device, image, context);
                }
                let result = apply(&mut device, &forced(request.clone()));
                assert!(result.is_ok(), "{context:?}: {result:?}");
            }
        }
    }
    assert!(refusals > 1000);
}

/// The one defect of the C program that is documented and not reproduced:
/// when `BLKGETSIZE` succeeds and `HDIO_GETGEO` fails, it reads a geometry
/// struct nobody filled in. The port treats the start sector and head count
/// as zero, so such a device is a whole disk with no known geometry.
#[test]
fn unfilled_geometry_is_read_as_zero() {
    let fat16 = Image::fixture("fat16");
    assert_eq!(
        apply(&mut fat16.device(Profile::SIZE_ONLY), &write(Kind::Mbr2000)),
        wrote(Kind::Mbr2000)
    );
    assert_eq!(
        apply(&mut fat16.device(Profile::SIZE_ONLY), &write(Kind::Fat16)),
        refused(Kind::Fat16, Unsuitable::NotPartition)
    );
    assert_eq!(
        apply(
            &mut fat16.device(Profile::SIZE_ONLY),
            &partition(Heads::Kernel)
        ),
        Ok(Report {
            partition: Some(Err(NotWritten)),
            ..Report::default()
        })
    );
}

// Failed writes.

/// Each write that fails is reported as such, and the call goes on to the
/// writes that remain.
#[test]
fn failed_writes_are_reported_one_by_one() {
    let fat16 = Image::fixture("fat16");
    let mut device = Faulty::new(&fat16, Profile::PARTITION, |_| true);
    let request = Request {
        partition: Some(Heads::Kernel),
        oem_id: Some(b"NEWOEM".to_vec()),
        disk_signature: Some(0xcafe),
        ..forced(write(Kind::Fat16))
    };
    let report = apply(&mut device, &request).unwrap();
    assert_eq!(
        report,
        Report {
            partition: Some(Err(NotWritten)),
            record: Some(RecordReport {
                kind: Kind::Fat16,
                written: Err(NotWritten),
                exfat_checksum: None,
            }),
            oem_id: Some(Err(NotWritten)),
            disk_signature: Some(Err(NotWritten)),
        }
    );
    assert!(!report.succeeded());
    assert_untouched(&device.inner, &fat16, "nothing can be written");

    // A record stops at the first array that fails.
    let mut device = Faulty::new(&fat16, Profile::PARTITION, |pos| pos == 0x3e);
    let report = apply(&mut device, &write(Kind::Fat16)).unwrap();
    assert_eq!(report.record.unwrap().written, Err(NotWritten));
    assert!(!report.succeeded());
    assert_image(
        &device.inner.bytes,
        &patched(&fat16.bytes, &[(0x0, p::BR_FAT16_0X0)]),
        "second array fails",
    );

    for (fails, written) in [
        (
            (|pos| pos == 0x1e00) as fn(u64) -> bool,
            Report {
                record: record_written(Kind::Mbr2000),
                oem_id: Some(Ok(())),
                disk_signature: Some(Ok(())),
                ..Report::default()
            },
        ),
        (
            |pos| pos == 0x3,
            Report {
                record: record_written(Kind::Mbr2000),
                oem_id: Some(Err(NotWritten)),
                disk_signature: Some(Ok(())),
                ..Report::default()
            },
        ),
        (
            |pos| pos == 0x1b8,
            Report {
                record: record_written(Kind::Mbr2000),
                oem_id: Some(Ok(())),
                disk_signature: Some(Err(NotWritten)),
                ..Report::default()
            },
        ),
    ] {
        let mut device = Faulty::new(&fat16, Profile::DISK, fails);
        let request = Request {
            oem_id: Some(b"NEWOEM".to_vec()),
            disk_signature: Some(0xcafe),
            ..forced(write(Kind::Mbr2000))
        };
        let report = apply(&mut device, &request).unwrap();
        assert_eq!(report, written);
        assert_eq!(report.succeeded(), fails(0x1e00));
    }
}

/// UPSTREAM-BUG: once the start sector is written, a head count or drive
/// ID that fails is reported and does not make the call a failure.
#[test]
fn partition_information_succeeds_with_its_start_sector() {
    let fat16 = Image::fixture("fat16");
    let report = |heads, drive_id| PartitionReport {
        start_sector: 2048,
        heads: Some(heads),
        drive_ids: vec![(Family::Fat16, drive_id)],
        exfat_checksum: None,
    };
    let start: Patch = (0x1c, &[0x00, 0x08, 0x00, 0x00]);
    let heads: Patch = (0x1a, &[0xff, 0x00]);
    let drive_id: Patch = (0x24, &[0x80]);
    let check = |fails: fn(u64) -> bool, partition_report, fields: &[Patch]| {
        let mut device = Faulty::new(&fat16, Profile::PARTITION, fails);
        let result = apply(&mut device, &partition(Heads::Kernel));
        assert_eq!(result, wrote_partition(partition_report));
        assert!(result.unwrap().succeeded());
        assert_image(
            &device.inner.bytes,
            &patched(&fat16.bytes, fields),
            "partition information",
        );
    };
    check(
        |pos| pos == 0x1a,
        report(Err(NotWritten), Ok(())),
        &[start, drive_id],
    );
    check(
        |pos| pos == 0x24,
        report(Ok(255), Err(NotWritten)),
        &[start, heads],
    );

    let mut device = Faulty::new(&fat16, Profile::PARTITION, |pos| pos == 0x1c);
    let result = apply(&mut device, &partition(Heads::Kernel));
    assert_eq!(
        result,
        Ok(Report {
            partition: Some(Err(NotWritten)),
            ..Report::default()
        })
    );
    assert_untouched(&device.inner, &fat16, "start sector fails");
}

/// UPSTREAM-BUG: the exFAT checksum is attempted, and may succeed, when
/// the boot record itself could not be written.
#[test]
fn the_exfat_checksum_follows_a_failed_record() {
    let exfat = Image::fixture("exfat");
    let mut device = Faulty::new(&exfat, Profile::PARTITION, |pos| pos == 0x78);
    let report = apply(&mut device, &write(Kind::ExfatNt6)).unwrap();
    assert_eq!(
        report.record,
        Some(RecordReport {
            kind: Kind::ExfatNt6,
            written: Err(NotWritten),
            exfat_checksum: Some(Ok(())),
        })
    );
    assert!(!report.succeeded());
    // The checksums the fixture already had, written again.
    assert_untouched(&device.inner, &exfat, "exfat");
}

// Automatic selection.

/// The automatic choice is the first record, in the order of upstream's
/// record type numbers, that would be written unforced. On a whole hard
/// disk that is always the DOS master boot record, whatever the disk holds.
#[test]
fn the_automatic_choice() {
    let cases = [
        ("fat12", Profile::FLOPPY, Some(Kind::Fat12)),
        ("fat12", Profile::FLOPPY_IOCTL_ONLY, Some(Kind::Fat12)),
        ("fat12", Profile::FLOPPY_PARTITION, Some(Kind::Fat12)),
        ("fat16", Profile::PARTITION, Some(Kind::Fat16)),
        ("fat16", Profile::FLOPPY_PARTITION, Some(Kind::Fat16)),
        ("fat32", Profile::PARTITION, Some(Kind::Fat32)),
        ("fat32", Profile::GEOMETRY_ONLY, Some(Kind::Fat32)),
        ("exfat", Profile::PARTITION, Some(Kind::ExfatNt6)),
        ("ntfs", Profile::PARTITION_4K, Some(Kind::Ntfs)),
        ("fat16+fat32", Profile::PARTITION, Some(Kind::Fat16)),
        ("fat32+exfat", Profile::PARTITION, Some(Kind::Fat32)),
        ("fat16", Profile::DISK, Some(Kind::MbrDos)),
        ("ntfs", Profile::DISK, Some(Kind::MbrDos)),
        ("zeros", Profile::DISK, Some(Kind::MbrDos)),
        ("empty", Profile::SIZE_ONLY, Some(Kind::MbrDos)),
        ("fat12", Profile::PARTITION, None),
        ("fat16", Profile::FLOPPY, None),
        ("zeros", Profile::PARTITION, None),
        ("zeros", Profile::FLOPPY, None),
        ("fat16", Profile::FILE, None),
        ("ntfs", Profile::FILE, None),
    ];
    for (name, profile, choice) in cases {
        let device = super::harness::image(name).device(profile);
        assert_eq!(smart_select(&device), choice, "{name} {}", profile.name);
    }
}

// Inspection.

fn diagnosis(file_systems: &[Family], record: BootRecord, oem_id: Option<&[u8; 8]>) -> Diagnosis {
    Diagnosis {
        file_systems: file_systems.to_vec(),
        boot_sector: Some(BootSector {
            record,
            oem_id: oem_id.copied().map(OemId),
        }),
    }
}

fn unsigned(file_systems: &[Family]) -> Diagnosis {
    Diagnosis {
        file_systems: file_systems.to_vec(),
        boot_sector: None,
    }
}

/// Freshly formatted file systems are known by their magic strings; their
/// boot records are nobody's this program knows.
#[test]
fn file_systems_are_recognised() {
    for (fixture, family, oem_id) in [
        ("fat12", Family::Fat12, b"mkfs.fat"),
        ("fat16", Family::Fat16, b"mkfs.fat"),
        ("fat32", Family::Fat32, b"mkfs.fat"),
        ("ntfs", Family::Ntfs, b"NTFS    "),
    ] {
        for profile in [Profile::FILE, Profile::PARTITION_4K] {
            assert_eq!(
                inspect(&Image::fixture(fixture).device(profile)),
                diagnosis(&[family], BootRecord::Unknown, Some(oem_id)),
                "{fixture}"
            );
        }
    }

    let both = Image::fixture("fat16").with("fat32", 0x52, b"FAT32   ");
    assert_eq!(
        inspect(&both.device(Profile::FILE)),
        diagnosis(
            &[Family::Fat16, Family::Fat32],
            BootRecord::Unknown,
            Some(b"mkfs.fat")
        )
    );
}

/// An exFAT boot record is one with a boot signature on each of its first
/// nine sectors, of the size the device reports.
#[test]
fn a_foreign_exfat_record_is_signed_on_nine_sectors() {
    let exfat = Image::fixture("exfat");
    let foreign = diagnosis(
        &[Family::Exfat],
        BootRecord::ForeignExfat,
        Some(b"EXFAT   "),
    );
    let unknown = diagnosis(&[Family::Exfat], BootRecord::Unknown, Some(b"EXFAT   "));
    assert_eq!(inspect(&exfat.device(Profile::FILE)), foreign);
    assert_eq!(inspect(&exfat.device(Profile::PARTITION)), foreign);
    assert_eq!(inspect(&exfat.device(Profile::PARTITION_4K)), unknown);
    assert_eq!(
        inspect(&exfat.device(Profile::PARTITION_0_BYTE_SECTORS)),
        unknown
    );
    for sector in 1..9 {
        let image = exfat.with("unsigned", (sector + 1) * 512 - 2, &[0, 0]);
        assert_eq!(inspect(&image.device(Profile::FILE)), unknown, "{sector}");
    }
    let image = exfat.with("unsigned", 10 * 512 - 2, &[0, 0]);
    assert_eq!(inspect(&image.device(Profile::FILE)), foreign);
    // The signatures alone do not make it one.
    let image = exfat.with("oem", 0x3, b"NOTEXFAT");
    assert_eq!(
        inspect(&image.device(Profile::FILE)),
        diagnosis(&[], BootRecord::Unknown, Some(b"NOTEXFAT"))
    );
}

/// Boot records this program does not write but has a name for.
#[test]
fn foreign_boot_records_are_recognised() {
    let signed = Image::signed();
    let sectors = |image: &Image| {
        image
            .with("sig2", 0x3fe, SIGNATURE)
            .with("sig3", 0x5fe, SIGNATURE)
    };
    let fat16 = Image::fixture("fat16");
    let fat32 = Image::fixture("fat32");
    let cases: [(Image, Diagnosis); 12] = [
        // A FAT16 boot record is a signed sector with Windows 9x's OEM ID.
        (
            fat16.with("mswin", 0x3, b"MSWIN4.1"),
            diagnosis(&[Family::Fat16], BootRecord::ForeignFat, Some(b"MSWIN4.1")),
        ),
        (
            fat16.with("msdos", 0x3, b"MSDOS5.0"),
            diagnosis(&[Family::Fat16], BootRecord::Unknown, Some(b"MSDOS5.0")),
        ),
        // A FAT32 one is three signed sectors with either OEM ID.
        (
            sectors(&fat32).with("msdos", 0x3, b"MSDOS5.0"),
            diagnosis(&[Family::Fat32], BootRecord::ForeignFat, Some(b"MSDOS5.0")),
        ),
        (
            sectors(&fat32)
                .with("msdos", 0x3, b"MSDOS5.0")
                .with("unsigned", 0x5fe, &[0, 0]),
            diagnosis(&[Family::Fat32], BootRecord::Unknown, Some(b"MSDOS5.0")),
        ),
        (
            signed.with("lilo", 0x6, b"LILO"),
            diagnosis(&[], BootRecord::Lilo, None),
        ),
        (
            signed.with("lilo-floppy", 0x2, b"LILO"),
            diagnosis(&[], BootRecord::Lilo, None),
        ),
        (
            signed.with("dos-f2", 0x0, p::MBR_DOS_F2),
            diagnosis(&[], BootRecord::MbrDosF2 { disk_signature: 0 }, None),
        ),
        (
            signed.with("dos-f2", 0x0, p::MBR_DOS_F2).with(
                "disk-signature",
                0x1b8,
                &[0x78, 0x56, 0x34, 0x12],
            ),
            diagnosis(
                &[],
                BootRecord::MbrDosF2 {
                    disk_signature: 0x1234_5678,
                },
                None,
            ),
        ),
        // A zeroed master boot record is zero up to the partition table.
        (
            signed.with("partition-table", 0x1be, &[0x80, 0x20, 0x21, 0x00]),
            diagnosis(&[], BootRecord::Exact(Kind::MbrZero), None),
        ),
        // Zero up to the disk signature is nearly one.
        (
            signed.with(
                "disk-signature",
                0x1b8,
                &[0x78, 0x56, 0x34, 0x12, 0x5a, 0x5a],
            ),
            diagnosis(
                &[],
                BootRecord::MbrZeroWith {
                    disk_signature: 0x1234_5678,
                    copy_protect: 0x5a5a,
                },
                None,
            ),
        ),
        (
            signed.with("copy-protect", 0x1bc, &[0x01, 0x02]),
            diagnosis(
                &[],
                BootRecord::MbrZeroWith {
                    disk_signature: 0,
                    copy_protect: 0x0201,
                },
                None,
            ),
        ),
        (
            signed.with("code-byte", 0x1b7, &[0x01]),
            diagnosis(&[], BootRecord::Unknown, None),
        ),
    ];
    for (image, expected) in cases {
        assert_eq!(
            inspect(&image.device(Profile::FILE)),
            expected,
            "{}",
            image.name
        );
    }
}

/// Without a boot signature there is no boot sector to describe, though
/// the file system magic is still looked for. Images too short to hold one
/// have neither.
#[test]
fn an_unsigned_sector_is_no_boot_sector() {
    let cases = [
        (Image::zeros(), unsigned(&[])),
        (
            Image::fixture("fat16").with("unsigned", 0x1fe, &[0, 0]),
            unsigned(&[Family::Fat16]),
        ),
        (Image::zeros().with("dos", 0x0, p::MBR_DOS), unsigned(&[])),
        (Image::new("short-511", vec![0; 511]), unsigned(&[])),
        (Image::new("short-100", vec![0x55; 100]), unsigned(&[])),
        (Image::new("empty", Vec::new()), unsigned(&[])),
        (
            Image::new("fat16-cut", Image::fixture("fat16").bytes[..0x1ff].to_vec()),
            unsigned(&[Family::Fat16]),
        ),
    ];
    for (image, expected) in cases {
        assert_eq!(
            inspect(&image.device(Profile::FILE)),
            expected,
            "{}",
            image.name
        );
    }
}

/// UPSTREAM-BUG: an OEM ID is given only if every byte of it is at least a
/// space as a signed `char`, so DEL passes and anything from 0x80 up does
/// not.
#[test]
fn an_oem_id_is_shown_if_printable() {
    let image = Image::fixture("fat16");
    let cases: [(&[u8; 8], bool); 6] = [
        (b"MSWIN4.1", true),
        (b"        ", true),
        (b"del \x7f\x7f  ", true),
        (b"caf\xc3\xa9   ", false),
        (b"tab\t    ", false),
        (b"nul\0    ", false),
    ];
    for (id, shown) in cases {
        let device = image.with("oem", 0x3, id).device(Profile::FILE);
        let oem_id = inspect(&device).boot_sector.unwrap().oem_id;
        assert_eq!(oem_id, shown.then_some(OemId(*id)), "{:?}", OemId(*id));
    }
}

/// What a record is recognised as once it is written, on an image that
/// held nothing and on a formatted FAT32 volume. Master boot records are
/// told with the disk signature beside them.
///
/// UPSTREAM-BUG: the FAT16 ReactOS record carries its own OEM ID and is
/// never recognised. The FAT32 NT6.0 record carries `MSDOS5.0`, which only
/// counts with all three sectors signed, and it signs but two of them, so
/// it is recognised on a FAT32 volume and not on a blank one.
#[test]
fn written_records_are_recognised() {
    use Kind::*;
    let zeros = Image::zeros();
    let fat32 = Image::fixture("fat32");
    for record in &RECORDS {
        let kind = record.kind;
        let expected = match (record.family, kind) {
            (Family::Mbr, MbrZero) => BootRecord::Exact(MbrZero),
            (Family::Mbr, _) => BootRecord::Mbr {
                kind,
                disk_signature: 0,
            },
            (_, Fat16Ros) => BootRecord::Unknown,
            (_, Fat32Nt6) => BootRecord::Unknown,
            _ => BootRecord::Exact(kind),
        };
        let mut device = zeros.device(Profile::FILE);
        apply(&mut device, &forced(write(kind))).unwrap();
        let found = inspect(&device).boot_sector.unwrap().record;
        assert_eq!(found, expected, "{kind:?} on zeros");

        if record.family == Family::Fat32 {
            let mut device = fat32.device(Profile::PARTITION);
            apply(&mut device, &write(kind)).unwrap();
            let found = inspect(&device).boot_sector.unwrap().record;
            assert_eq!(found, BootRecord::Exact(kind), "{kind:?} on fat32");
        }
    }
}

/// A record stays recognised when what is written beside it changes, and
/// stops being when it is written into.
#[test]
fn written_records_are_recognised_until_changed() {
    let record_after = |image: &Image, profile, first: Request, then: Request| {
        let mut device = image.device(profile);
        apply(&mut device, &forced(first)).unwrap();
        apply(&mut device, &forced(then)).unwrap();
        inspect(&device).boot_sector.unwrap().record
    };
    let zeros = Image::zeros();
    let fat32 = Image::fixture("fat32");
    let exfat = Image::fixture("exfat");

    assert_eq!(
        record_after(
            &zeros,
            Profile::DISK,
            write(Kind::MbrWin7),
            disk_signature(0x89ab_cdef)
        ),
        BootRecord::Mbr {
            kind: Kind::MbrWin7,
            disk_signature: 0x89ab_cdef
        }
    );
    assert_eq!(
        record_after(
            &zeros,
            Profile::DISK,
            write(Kind::MbrZero),
            disk_signature(0x89ab_cdef)
        ),
        BootRecord::MbrZeroWith {
            disk_signature: 0x89ab_cdef,
            copy_protect: 0
        }
    );
    // The OEM ID lies inside the first array of every boot record.
    assert_eq!(
        record_after(
            &zeros,
            Profile::DISK,
            write(Kind::MbrWin7),
            oem_id(b"NEWOEM")
        ),
        BootRecord::Unknown
    );
    assert_eq!(
        record_after(
            &fat32,
            Profile::PARTITION,
            write(Kind::Fat32),
            oem_id(b"MSDOS5.0")
        ),
        BootRecord::ForeignFat
    );
    assert_eq!(
        record_after(
            &fat32,
            Profile::PARTITION,
            write(Kind::Fat32),
            oem_id(b"NEWOEM")
        ),
        BootRecord::Unknown
    );
    // Partition information and the label lie between the arrays.
    assert_eq!(
        record_after(
            &fat32,
            Profile::PARTITION,
            write(Kind::Fat32Pe),
            partition(Heads::Kernel)
        ),
        BootRecord::Exact(Kind::Fat32Pe)
    );
    assert_eq!(
        record_after(
            &exfat,
            Profile::PARTITION,
            write(Kind::ExfatNt6),
            partition(Heads::Kernel)
        ),
        BootRecord::Exact(Kind::ExfatNt6)
    );
    // The backup of the exFAT record is looked for where sectors of the
    // device's size put it.
    let mut device = exfat.device(Profile::PARTITION);
    apply(&mut device, &write(Kind::ExfatNt6)).unwrap();
    device.probe = Profile::PARTITION_4K.probe;
    assert_eq!(
        inspect(&device).boot_sector.unwrap().record,
        BootRecord::Unknown
    );
    assert_eq!(IMAGE_SIZE, device.bytes.len());
}
