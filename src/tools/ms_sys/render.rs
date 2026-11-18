use std::io::Write;

use super::apply::{PartitionReport, Refusal, Report, Written};
use super::cli::Invocation;
use super::identify::{BootRecord, Diagnosis, OemIdUnsuitable, Unsuitable};
use super::record::{Family, Kind};

pub fn nothing_selected(out: &mut dyn Write, path: &[u8]) {
    put!(
        out,
        "Unable to automaticly select boot record for ",
        path,
        "\n"
    );
}

pub fn unopenable(out: &mut dyn Write, path: &[u8], reason: &[u8]) {
    put!(out, "Unable to open ", path, ", ", reason, "\n");
}

fn unsuitable(out: &mut dyn Write, path: &[u8], kind: Kind, reason: Unsuitable) {
    let family = kind.record().family;
    let name = family.file_system();
    let complaint = match reason {
        Unsuitable::NotDiskDevice => "does not seem to be a disk device".to_owned(),
        Unsuitable::Floppy => "seems to be a floppy disk device".to_owned(),
        Unsuitable::Partition => "seems to be a disk partition device".to_owned(),
        Unsuitable::NotFloppy => "does not seem to be a floppy disk device".to_owned(),
        Unsuitable::NotPartition => "does not seem to be a disk partition device".to_owned(),
        Unsuitable::NoFileSystem => format!("does not seem to have a {name} file system"),
    };
    let record = match family {
        Family::Mbr => "master",
        _ => name,
    };
    put!(out, path, " ", complaint, ",\n");
    put!(
        out,
        "use the switch -f to force writing of a ",
        record,
        " boot record\n"
    );
}

pub fn refusal(out: &mut dyn Write, path: &[u8], refusal: Refusal) {
    match refusal {
        Refusal::Record { kind, reason } => unsuitable(out, path, kind, reason),
        Refusal::DiskSignature(reason) => unsuitable(out, path, Kind::MbrZero, reason),
        Refusal::OemId(reason) => {
            match reason {
                OemIdUnsuitable::WrongDevice => put!(
                    out,
                    path,
                    " does not seem to be a floppy or disk partition device,\n"
                ),
                OemIdUnsuitable::NoFatFileSystem => {
                    put!(out, path, " does not seem to have a FAT file system,\n");
                }
            }
            put!(out, "use the switch -f to force writing of OEM ID\n");
        }
        Refusal::PartitionWithoutFileSystem => {
            put!(out, "Failed writing partition information to ", path, "\n");
            put!(
                out,
                "Device must be formatted as FAT16, FAT32, EXFAT or NTFS\n"
            );
        }
    }
}

pub fn report(
    out: &mut dyn Write,
    err: &mut dyn Write,
    path: &[u8],
    invocation: &Invocation,
    report: &Report,
) {
    match &report.partition {
        None => {}
        Some(Err(_)) => {
            put!(
                err,
                "Failed writing start sector to ",
                path,
                ", this is only possible to do with\n"
            );
            put!(err, "real partitions!\n");
        }
        Some(Ok(partition)) => partition_report(out, err, path, partition),
    }
    if let Some(outcome) = report.record {
        let record = outcome.kind.record();
        match outcome.written {
            Ok(()) => put!(out, record.written, " successfully written to ", path, "\n"),
            Err(_) => put!(err, "Failed writing ", record.failed, " to ", path, "\n"),
        }
        if let Some(checksum) = outcome.exfat_checksum {
            exfat_checksum(out, err, path, checksum);
        }
    }
    if let (Some(outcome), Some(oem_id)) = (report.oem_id, &invocation.oem_id) {
        match outcome {
            // The ID is cut or padded to eight bytes on disk, but reported
            // as given.
            Ok(()) => put!(
                out,
                "OEM ID '",
                oem_id.as_encoded_bytes(),
                "' successfully written to ",
                path,
                "\n"
            ),
            Err(_) => put!(err, "Failed writing OEM ID to ", path, "\n"),
        }
    }
    if let (Some(outcome), Some(signature)) = (report.disk_signature, invocation.disk_signature) {
        match outcome {
            Ok(()) => {
                put!(
                    out,
                    format!("Windows Disk Signature 0x{signature:08x} successfully written to ")
                );
                put!(out, path, "\n");
            }
            Err(_) => put!(err, "Failed writing Windows Disk Signature to ", path, "\n"),
        }
    }
}

fn partition_report(
    out: &mut dyn Write,
    err: &mut dyn Write,
    path: &[u8],
    partition: &PartitionReport,
) {
    put!(
        out,
        format!(
            "Start sector {} (nr of hidden sectors) successfully written to ",
            partition.start_sector.cast_signed()
        )
    );
    put!(out, path, "\n");
    match partition.heads {
        None => {}
        Some(Ok(heads)) => put!(
            out,
            format!("Number of heads ({heads}) successfully written to "),
            path,
            "\n"
        ),
        Some(Err(_)) => put!(err, "Failed writing number of heads to ", path, "\n"),
    }
    for &(family, outcome) in &partition.drive_ids {
        match outcome {
            Ok(()) => put!(
                out,
                "Physical disk drive id 0x80 (C:) successfully written to ",
                path,
                "\n"
            ),
            Err(_) => put!(err, "Failed writing physical disk drive id to ", path, "\n"),
        }
        if let (Family::Exfat, Some(checksum)) = (family, partition.exfat_checksum) {
            exfat_checksum(out, err, path, checksum);
        }
    }
}

fn exfat_checksum(out: &mut dyn Write, err: &mut dyn Write, path: &[u8], outcome: Written) {
    match outcome {
        Ok(()) => put!(
            out,
            "EXFAT VBR checksum successfully written to ",
            path,
            "\n"
        ),
        Err(_) => put!(err, "Failed writing EXFAT VBR checksum to ", path, "\n"),
    }
}

pub fn diagnosis(out: &mut dyn Write, path: &[u8], diagnosis: &Diagnosis) {
    for family in &diagnosis.file_systems {
        put!(
            out,
            path,
            " has a ",
            family.file_system(),
            " file system.\n"
        );
    }
    let Some(boot_sector) = diagnosis.boot_sector else {
        put!(out, path, " has no x86 boot sector\n");
        return;
    };
    put!(out, path, " has an x86 boot sector,\n");
    boot_record(out, boot_sector.record);
    if let Some(id) = boot_sector.oem_id {
        put!(out, "The OEM ID is ", id.0, "\n");
    }
}

fn boot_record(out: &mut dyn Write, record: BootRecord) {
    let signature = |out: &mut dyn Write, disk_signature: u32| {
        put!(
            out,
            format!("It has windows disk signature 0x{disk_signature:08x}.\n")
        );
    };
    match record {
        BootRecord::Exact(Kind::MbrZero) => {
            put!(
                out,
                "it is a zeroed non-bootable master boot record, like the one this\n"
            );
            put!(
                out,
                "program creates with the switch -z on a hard disk device.\n"
            );
        }
        BootRecord::Exact(kind) => {
            let record = kind.record();
            let target = match record.family {
                Family::Fat12 => "floppy".to_owned(),
                family => format!("{} partition", family.file_system()),
            };
            put!(
                out,
                "it is exactly the kind of ",
                record.described,
                " boot record this program\n"
            );
            put!(
                out,
                format!(
                    "would create with the switch -{} on a {target}.\n",
                    record.short
                )
            );
        }
        BootRecord::Mbr {
            kind,
            disk_signature,
        } => {
            let record = kind.record();
            put!(
                out,
                "it is a ",
                record.described,
                " master boot record, like the one this\n"
            );
            put!(
                out,
                format!(
                    "program creates with the switch -{} on a hard disk device.\n",
                    record.short
                )
            );
            signature(out, disk_signature);
        }
        BootRecord::MbrDosF2 { disk_signature } => {
            put!(
                out,
                "it is a Microsoft DOS/NT/95A master boot record with the undocumented\n"
            );
            put!(
                out,
                "F2 instruction. You will get equal functionality with the MBR this\n"
            );
            put!(
                out,
                "program creates with the switch -d on a hard disk device.\n"
            );
            signature(out, disk_signature);
        }
        BootRecord::MbrZeroWith {
            disk_signature,
            copy_protect,
        } => {
            let explained = match copy_protect {
                0 => "not copy protected",
                0x5a5a => "copy protected",
                _ => "unknown value",
            };
            put!(
                out,
                "it is a non-bootable master boot record, almost like the one this\n"
            );
            put!(
                out,
                "program creates with the switch -z on a hard disk device but\n"
            );
            put!(
                out,
                format!("with windows disk signature 0x{disk_signature:08x}\n")
            );
            put!(
                out,
                "which this program can write with switch -S and copy protect\n"
            );
            put!(out, format!("bytes 0x{copy_protect:04x} ({explained}).\n"));
        }
        BootRecord::ForeignFat => {
            // UPSTREAM-BUG: the list of switches was never extended beyond
            // the first five FAT records.
            put!(out, "it seems to be a FAT16 or FAT32 boot record, but it\n");
            put!(
                out,
                "differs from what this program would create with the\n"
            );
            put!(
                out,
                "switch -6, -2, -8, -e or -3 on a FAT16 or FAT32 partition.\n"
            );
        }
        BootRecord::ForeignExfat => {
            put!(
                out,
                "it seems to be a EXFAT boot record, but it differs from\n"
            );
            put!(
                out,
                "what this program would create with the switch -x on a\n"
            );
            put!(out, "EXFAT partition.\n");
        }
        BootRecord::Lilo => {
            put!(
                out,
                "it seems to be a LILO boot record, please use lilo to\n"
            );
            put!(out, "create such boot records.\n");
        }
        BootRecord::Unknown => put!(out, "it is an unknown boot record\n"),
    }
}
