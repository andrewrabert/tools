//! The command-line edge: what the parser makes of an argument vector,
//! what the program prints and exits with, and how it opens a device by
//! path. What the library then does to the device is tested in `api.rs`.

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use clap::Parser;

use super::super::cli::{Invocation, Selection};
use super::super::device::Device;
use super::super::payloads::MBR_2000;
use super::super::record::{Kind, RECORDS};
use super::harness::{DEVICE, Faulty, Image, Profile, assert_image, assert_untouched, patched};

fn words(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

/// An argument that need not be valid UTF-8.
fn raw(bytes: &[u8]) -> OsString {
    OsString::from_vec(bytes.to_vec())
}

/// Runs the program as `ms-sys` with `arguments`.
fn run(arguments: &[OsString]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let status = match parse(&command_line(arguments)) {
        Ok(invocation) => i32::from(super::super::run(&invocation, &mut out, &mut err)),
        Err(error) => {
            let rendered = error.render().to_string().into_bytes();
            if error.use_stderr() {
                err = rendered;
            } else {
                out = rendered;
            }
            error.exit_code()
        }
    };
    (status, out, err)
}

/// The tool as bertbox runs it under its own name.
#[derive(Parser)]
#[command(multicall = true, version = crate::APP_VERSION, propagate_version = true)]
enum Program {
    #[command(name = "ms-sys")]
    MsSys(Invocation),
}

/// Parses an argument vector, program name first. The error is also how
/// clap hands back the text of `--help` and `--version`.
fn parse(args: &[OsString]) -> Result<Invocation, clap::Error> {
    Program::try_parse_from(args).map(|Program::MsSys(invocation)| invocation)
}

fn command_line(arguments: &[OsString]) -> Vec<OsString> {
    let mut all = vec![OsString::from("ms-sys")];
    all.extend_from_slice(arguments);
    all
}

// The parser.

#[track_caller]
fn parsed(arguments: &[OsString]) -> Invocation {
    match parse(&command_line(arguments)) {
        Ok(invocation) => invocation,
        Err(error) => panic!("{arguments:?}: {error}"),
    }
}

/// A device alone asks for nothing to be written.
#[test]
fn a_device_alone_inspects() {
    let invocation = parsed(&words(&[DEVICE]));
    assert_eq!(invocation.selection(), None);
    assert!(!invocation.force);
    assert!(!invocation.wipe_label);
    assert!(!invocation.partition);
    assert_eq!(invocation.heads, None);
    assert_eq!(invocation.bytes_per_sector, 512);
    assert_eq!(invocation.oem_id, None);
    assert_eq!(invocation.disk_signature, None);
    assert_eq!(invocation.device, Path::new(DEVICE));
    assert!(!invocation.writes());
}

/// The device is the one positional argument, wherever it stands, and may
/// be any bytes; after `--` it may look like a switch.
#[test]
fn the_device_is_the_positional_argument() {
    for arguments in [
        &["-m", "-f", DEVICE][..],
        &[DEVICE, "-m", "-f"],
        &["-m", DEVICE, "-f"],
        &["-mf", "--", DEVICE],
    ] {
        let invocation = parsed(&words(arguments));
        assert_eq!(invocation.device, Path::new(DEVICE), "{arguments:?}");
        assert_eq!(
            invocation.selection(),
            Some(Selection::Record(Kind::Mbr2000)),
            "{arguments:?}"
        );
        assert!(invocation.force, "{arguments:?}");
    }
    let invocation = parsed(&words(&["--", "-m"]));
    assert_eq!(invocation.device, Path::new("-m"));
    assert_eq!(invocation.selection(), None);
    let odd = raw(b"missing-\xff.img");
    assert_eq!(parsed(std::slice::from_ref(&odd)).device, Path::new(&odd));
}

/// Each row of `RECORDS` has its switch, short and long, in the parser.
#[test]
fn every_record_has_its_switches() {
    for record in &RECORDS {
        for switch in [format!("-{}", record.short), format!("--{}", record.long)] {
            let invocation = parsed(&words(&[&switch, DEVICE]));
            assert_eq!(
                invocation.selection(),
                Some(Selection::Record(record.kind)),
                "{switch}"
            );
            assert!(invocation.writes(), "{switch}");
        }
    }
    for switch in ["-w", "--write"] {
        let invocation = parsed(&words(&[switch, DEVICE]));
        assert_eq!(invocation.selection(), Some(Selection::Auto), "{switch}");
    }
}

/// Every switch that is not a record type, in each way clap spells it.
#[test]
fn switches_beside_the_record_type() {
    for arguments in [
        &[
            "-w", "-f", "-l", "-p", "-H", "16", "-B", "4096", "-O", "NEWOEM", "-S", "cafe", DEVICE,
        ][..],
        &[
            "--write",
            "--force",
            "--wipelabel",
            "--partition",
            "--heads",
            "16",
            "--bps",
            "4096",
            "--writeoem",
            "NEWOEM",
            "--writewds",
            "cafe",
            DEVICE,
        ],
        &[
            "-wflpH16",
            "--bps=4096",
            "-ONEWOEM",
            "--writewds=0xCAFE",
            DEVICE,
        ],
    ] {
        let invocation = parsed(&words(arguments));
        assert_eq!(invocation.selection(), Some(Selection::Auto));
        assert!(invocation.force);
        assert!(invocation.wipe_label);
        assert!(invocation.partition);
        assert_eq!(invocation.heads, Some(16));
        assert_eq!(invocation.bytes_per_sector, 4096);
        assert_eq!(invocation.oem_id, Some("NEWOEM".into()));
        assert_eq!(invocation.disk_signature, Some(0xcafe));
        assert_eq!(invocation.device, Path::new(DEVICE));
    }
}

/// Anything else that is to be written also opens the device for writing.
#[test]
fn what_counts_as_writing() {
    for arguments in [&["-p"][..], &["-O", "x"], &["-S", "1"]] {
        let mut arguments = words(arguments);
        arguments.push(DEVICE.into());
        assert!(parsed(&arguments).writes(), "{arguments:?}");
    }
    for arguments in [&["-f"][..], &["-l"], &["-H", "4"], &["-B", "4096"]] {
        let mut arguments = words(arguments);
        arguments.push(DEVICE.into());
        assert!(!parsed(&arguments).writes(), "{arguments:?}");
    }
}

/// An OEM ID is passed on untouched, bytes that are not UTF-8 included.
/// One that starts with a dash has to be attached to its switch.
#[test]
fn an_oem_id_may_be_any_bytes() {
    for id in [OsString::from(""), "a=b".into(), raw(b"\xff\xfe\x80oem")] {
        let invocation = parsed(&["-O".into(), id.clone(), DEVICE.into()]);
        assert_eq!(invocation.oem_id, Some(id));
    }
    for (argument, id) in [("--writeoem=--mbr", "--mbr"), ("-O-f", "-f")] {
        let invocation = parsed(&words(&[argument, DEVICE]));
        assert_eq!(invocation.oem_id, Some(id.into()), "{argument}");
        assert_eq!(invocation.selection(), None, "{argument}");
        assert!(!invocation.force, "{argument}");
    }
}

/// Numbers are read whole and must be in range.
#[test]
fn numbers() {
    for (arguments, heads) in [
        (&["-H", "1"][..], 1),
        (&["-H", "0016"], 16),
        (&["-H", "65535"], 65535),
    ] {
        let mut arguments = words(arguments);
        arguments.push(DEVICE.into());
        assert_eq!(parsed(&arguments).heads, Some(heads), "{arguments:?}");
    }
    for (arguments, bytes_per_sector) in [
        (&["-B", "512"][..], 512),
        (&["-B", "0512"], 512),
        (&["-B", "65536"], 65536),
    ] {
        let mut arguments = words(arguments);
        arguments.push(DEVICE.into());
        assert_eq!(
            parsed(&arguments).bytes_per_sector,
            bytes_per_sector,
            "{arguments:?}"
        );
    }
    for (arguments, signature) in [
        (&["-S", "0"][..], 0),
        (&["-S", "1234abcd"], 0x1234_abcd),
        (&["-S", "0xDEADBEEF"], 0xdead_beef),
        (&["-S", "0Xffffffff"], 0xffff_ffff),
        (&["-S", "000000001"], 1),
    ] {
        let mut arguments = words(arguments);
        arguments.push(DEVICE.into());
        assert_eq!(
            parsed(&arguments).disk_signature,
            Some(signature),
            "{arguments:?}"
        );
    }
}

/// A command line that cannot be parsed is an error: clap says why on
/// stderr, nothing goes to stdout, and the exit status is 2. Nothing is
/// opened; none of these devices exist.
#[test]
fn command_lines_that_are_refused() {
    let argument_lists: &[&[&str]] = &[
        // No device, or more than one.
        &[],
        &["-m"],
        &["--mbr", "-f"],
        &[DEVICE, DEVICE],
        &["stray", DEVICE],
        // A switch that lacks its value.
        &[DEVICE, "-H"],
        &[DEVICE, "-B"],
        &[DEVICE, "-O"],
        &[DEVICE, "-S"],
        &["-O", "-f", DEVICE],
        &["-O", "--mbr", DEVICE],
        // More than one record type, or one repeated.
        &["-m", "--fat12", DEVICE],
        &["-1", "-m", DEVICE],
        &["-w", "-m", DEVICE],
        &["-m", "-w", DEVICE],
        &["-z", "-7", "-i", "-9", "-d", DEVICE],
        &["-m", "-m", DEVICE],
        &["-w", "--write", DEVICE],
        // Any other switch repeated.
        &["-f", "--force", DEVICE],
        &["-H", "4", "-H", "8", DEVICE],
        &["-O", "a", "--writeoem", "b", DEVICE],
        // Unknown switches.
        &["--nonsense", DEVICE],
        &["--fat", DEVICE],
        &["--mbrx", DEVICE],
        &["--MBR", DEVICE],
        &["---mbr", DEVICE],
        &["--mbr=1", DEVICE],
        &["-Z", DEVICE],
        &["-M", DEVICE],
        &["-0", DEVICE],
        &["-v", DEVICE],
        &["-é", DEVICE],
        // Numbers that are not numbers, or out of range.
        &["-H", "x", DEVICE],
        &["-H", "", DEVICE],
        &["-H", "-1", DEVICE],
        &["-H", "1.5", DEVICE],
        &["-H", " 4", DEVICE],
        &["-H", "0", DEVICE],
        &["-H", "65536", DEVICE],
        &["-H", "4294967295", DEVICE],
        &["-B", "0x200", DEVICE],
        &["-B", "511", DEVICE],
        &["-B", "65537", DEVICE],
        &["-B", "99999999999999999999999", DEVICE],
        &["-S", "", DEVICE],
        &["-S", "0x", DEVICE],
        &["-S", "zz", DEVICE],
        &["-S", "12zz", DEVICE],
        &["-S", "+12", DEVICE],
        &["-S", " 12", DEVICE],
        &["-S", "-1", DEVICE],
        &["-S", "123456789", DEVICE],
    ];
    let mut command_lines: Vec<Vec<OsString>> =
        argument_lists.iter().map(|list| words(list)).collect();
    command_lines.extend([
        vec!["-H".into(), raw(b"1\xff"), DEVICE.into()],
        vec!["-S".into(), raw(b"1f\xff"), DEVICE.into()],
        vec![raw(b"--mbr\xff"), DEVICE.into()],
        vec![raw(b"-\xff"), DEVICE.into()],
    ]);
    for arguments in &command_lines {
        let (status, out, err) = run(arguments);
        let err = String::from_utf8_lossy(&err);
        assert_eq!(status, 2, "{arguments:?}");
        assert_eq!(out, b"", "{arguments:?}");
        assert!(err.starts_with("error: "), "{arguments:?}: {err}");
    }
}

// What the program prints.

/// Runs the program with `switches` on a device it has already opened and
/// returns its exit status and what it printed on stdout and on stderr.
#[track_caller]
fn run_with(device: &mut dyn Device, switches: &str) -> (i32, String, String) {
    let mut arguments: Vec<OsString> = switches.split_whitespace().map(OsString::from).collect();
    arguments.push(DEVICE.into());
    let invocation = parsed(&arguments);
    let mut out = Vec::new();
    let mut err = Vec::new();
    let status = i32::from(super::super::execute(
        &invocation,
        device,
        &mut out,
        &mut err,
    ));
    (
        status,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// Runs each of `runs` in turn on one device holding `image`, and checks
/// the exit status, stdout and stderr of each.
#[track_caller]
fn session(image: &Image, profile: Profile, runs: &[(&str, i32, &str, &str)]) {
    let mut device = image.device(profile);
    for &(switches, status, out, err) in runs {
        assert_eq!(
            run_with(&mut device, switches),
            (status, out.to_owned(), err.to_owned()),
            "{} {}: {switches:?}",
            image.name,
            profile.name
        );
    }
}

/// A refusal says on stderr what is wrong with the device and how to force
/// the write, exits with 1, and writes nothing.
#[test]
fn refusals() {
    let cases = [
        (
            "ntfs",
            Profile::FILE,
            "--mbr",
            "disk.img does not seem to be a disk device,\n\
             use the switch -f to force writing of a master boot record\n",
        ),
        (
            "ntfs",
            Profile::FLOPPY,
            "-7",
            "disk.img seems to be a floppy disk device,\n\
             use the switch -f to force writing of a master boot record\n",
        ),
        (
            "ntfs",
            Profile::PARTITION,
            "-7",
            "disk.img seems to be a disk partition device,\n\
             use the switch -f to force writing of a master boot record\n",
        ),
        (
            "fat12",
            Profile::DISK,
            "--fat12",
            "disk.img does not seem to be a floppy disk device,\n\
             use the switch -f to force writing of a FAT12 boot record\n",
        ),
        (
            "fat16",
            Profile::DISK,
            "-p -6",
            "disk.img does not seem to be a disk partition device,\n\
             use the switch -f to force writing of a FAT16 boot record\n",
        ),
        (
            "ntfs",
            Profile::FLOPPY,
            "-1",
            "disk.img does not seem to have a FAT12 file system,\n\
             use the switch -f to force writing of a FAT12 boot record\n",
        ),
        (
            "ntfs",
            Profile::PARTITION,
            "-5",
            "disk.img does not seem to have a FAT16 file system,\n\
             use the switch -f to force writing of a FAT16 boot record\n",
        ),
        (
            "ntfs",
            Profile::PARTITION,
            "-2",
            "disk.img does not seem to have a FAT32 file system,\n\
             use the switch -f to force writing of a FAT32 boot record\n",
        ),
        (
            "fat16",
            Profile::PARTITION,
            "-n",
            "disk.img does not seem to have a NTFS file system,\n\
             use the switch -f to force writing of a NTFS boot record\n",
        ),
        (
            "fat16",
            Profile::PARTITION,
            "-x",
            "disk.img does not seem to have a EXFAT file system,\n\
             use the switch -f to force writing of a EXFAT boot record\n",
        ),
        (
            "fat12",
            Profile::DISK,
            "-O MSWIN4.1",
            "disk.img does not seem to be a floppy or disk partition device,\n\
             use the switch -f to force writing of OEM ID\n",
        ),
        (
            "ntfs",
            Profile::DISK,
            "-O MSWIN4.1",
            "disk.img does not seem to have a FAT file system,\n\
             use the switch -f to force writing of OEM ID\n",
        ),
        (
            "fat16",
            Profile::FILE,
            "-S 1",
            "disk.img does not seem to be a disk device,\n\
             use the switch -f to force writing of a master boot record\n",
        ),
        (
            "fat12",
            Profile::PARTITION,
            "-p",
            "Failed writing partition information to disk.img\n\
             Device must be formatted as FAT16, FAT32, EXFAT or NTFS\n",
        ),
    ];
    for (fixture, profile, switches, output) in cases {
        let image = Image::fixture(fixture);
        let mut device = image.device(profile);
        assert_eq!(
            run_with(&mut device, switches),
            (1, String::new(), output.to_owned()),
            "{fixture} {}: {switches:?}",
            profile.name
        );
        assert_untouched(&device, &image, switches);
    }
}

/// Each thing written is reported on a line of its own, in the order it
/// was written: on stdout if it was, on stderr if it could not be.
#[test]
fn what_was_written() {
    session(
        &Image::fixture("ntfs"),
        Profile::DISK,
        &[
            (
                "-m",
                0,
                "Windows 2000/XP/2003 master boot record successfully written to disk.img\n",
                "",
            ),
            (
                "-S cafe -z",
                0,
                "Empty (zeroed) master boot record successfully written to disk.img\n\
                 Windows Disk Signature 0x0000cafe successfully written to disk.img\n",
                "",
            ),
            (
                "-n -f",
                0,
                "NTFS Windows 7 boot record successfully written to disk.img\n",
                "",
            ),
        ],
    );
    session(
        &Image::fixture("fat12"),
        Profile::FLOPPY,
        &[(
            "-O NEWOEM -w",
            0,
            "FAT12 boot record successfully written to disk.img\n\
             OEM ID 'NEWOEM' successfully written to disk.img\n",
            "",
        )],
    );
    // The OEM ID is named as given, though eight bytes of it were written.
    session(
        &Image::fixture("fat16"),
        Profile::PARTITION,
        &[(
            "-O TENLETTERS",
            0,
            "OEM ID 'TENLETTERS' successfully written to disk.img\n",
            "",
        )],
    );
    // Everything at once. On a whole disk the automatic choice is the DOS
    // master boot record and the partition information fails.
    session(
        &Image::fixture("ntfs"),
        Profile::DISK,
        &[(
            "-S cafe -O BOTH -p -w -f -l -H 8 -B 2048",
            1,
            "DOS/Windows NT master boot record successfully written to disk.img\n\
             OEM ID 'BOTH' successfully written to disk.img\n\
             Windows Disk Signature 0x0000cafe successfully written to disk.img\n",
            "Failed writing start sector to disk.img, this is only possible to do with\n\
             real partitions!\n",
        )],
    );
}

#[test]
fn partition_information_written() {
    session(
        &Image::fixture("fat16"),
        Profile::PARTITION,
        &[
            (
                "-p",
                0,
                "Start sector 2048 (nr of hidden sectors) successfully written to disk.img\n\
                 Number of heads (255) successfully written to disk.img\n\
                 Physical disk drive id 0x80 (C:) successfully written to disk.img\n",
                "",
            ),
            (
                "-p -H 16 -6",
                0,
                "Start sector 2048 (nr of hidden sectors) successfully written to disk.img\n\
                 Number of heads (16) successfully written to disk.img\n\
                 Physical disk drive id 0x80 (C:) successfully written to disk.img\n\
                 FAT16 boot record successfully written to disk.img\n",
                "",
            ),
        ],
    );
    // UPSTREAM-BUG: a head count that cannot be written is not held against
    // the rest.
    session(
        &Image::fixture("fat16"),
        Profile::PARTITION_0_HEADS,
        &[(
            "-p",
            0,
            "Start sector 2048 (nr of hidden sectors) successfully written to disk.img\n\
             Physical disk drive id 0x80 (C:) successfully written to disk.img\n",
            "Failed writing number of heads to disk.img\n",
        )],
    );
    session(
        &Image::fixture("ntfs"),
        Profile::PARTITION,
        &[(
            "-p",
            0,
            "Start sector 2048 (nr of hidden sectors) successfully written to disk.img\n\
             Number of heads (255) successfully written to disk.img\n",
            "",
        )],
    );
    // The whole start sector is named, though FAT holds 32 bits of it.
    session(
        &Image::fixture("fat32"),
        Profile::PARTITION_BEYOND_32_BITS,
        &[(
            "-p",
            0,
            "Start sector 4886718345 (nr of hidden sectors) successfully written to disk.img\n\
             Number of heads (255) successfully written to disk.img\n\
             Physical disk drive id 0x80 (C:) successfully written to disk.img\n",
            "",
        )],
    );
    session(
        &Image::fixture("fat12"),
        Profile::DISK,
        &[(
            "-p -3 -f",
            1,
            "FAT32 DOS boot record successfully written to disk.img\n",
            "Failed writing start sector to disk.img, this is only possible to do with\n\
             real partitions!\n",
        )],
    );
}

/// The exFAT checksum has a line of its own. Failing it fails a record,
/// but not partition information.
#[test]
fn the_exfat_checksum_is_reported() {
    let exfat = Image::fixture("exfat");
    session(
        &exfat,
        Profile::PARTITION,
        &[
            (
                "-x",
                0,
                "EXFAT NT6.0 boot record successfully written to disk.img\n\
                 EXFAT VBR checksum successfully written to disk.img\n",
                "",
            ),
            (
                "-p",
                0,
                "Start sector 2048 (nr of hidden sectors) successfully written to disk.img\n\
                 Physical disk drive id 0x80 (C:) successfully written to disk.img\n\
                 EXFAT VBR checksum successfully written to disk.img\n",
                "",
            ),
        ],
    );
    session(
        &exfat,
        Profile::PARTITION_0_BYTE_SECTORS,
        &[
            (
                "-x",
                1,
                "EXFAT NT6.0 boot record successfully written to disk.img\n",
                "Failed writing EXFAT VBR checksum to disk.img\n",
            ),
            (
                "-p",
                0,
                "Start sector 2048 (nr of hidden sectors) successfully written to disk.img\n\
                 Physical disk drive id 0x80 (C:) successfully written to disk.img\n",
                "Failed writing EXFAT VBR checksum to disk.img\n",
            ),
        ],
    );
}

/// Writes that fail are each reported on stderr, and the program exits
/// with 1.
#[test]
fn failed_writes() {
    let fat16 = Image::fixture("fat16");
    let mut device = Faulty::new(&fat16, Profile::PARTITION, |_| true);
    assert_eq!(
        run_with(&mut device, "-p -6 -O NEWOEM -S cafe -f"),
        (
            1,
            String::new(),
            "Failed writing start sector to disk.img, this is only possible to do with\n\
             real partitions!\n\
             Failed writing FAT16 boot record to disk.img\n\
             Failed writing OEM ID to disk.img\n\
             Failed writing Windows Disk Signature to disk.img\n"
                .to_owned()
        )
    );
    assert_eq!(
        run_with(&mut device, "-z -f"),
        (
            1,
            String::new(),
            "Failed writing empty (zeroed) master boot record to disk.img\n".to_owned()
        )
    );
    assert_eq!(
        run_with(&mut device, "-b -f"),
        (
            1,
            String::new(),
            "Failed writing GRUB2 master boot record to disk.img\n".to_owned()
        )
    );

    let mut device = Faulty::new(&fat16, Profile::PARTITION, |pos| pos == 0x24);
    assert_eq!(
        run_with(&mut device, "-p"),
        (
            0,
            "Start sector 2048 (nr of hidden sectors) successfully written to disk.img\n\
             Number of heads (255) successfully written to disk.img\n"
                .to_owned(),
            "Failed writing physical disk drive id to disk.img\n".to_owned()
        )
    );
}

/// When no record can be chosen automatically, that is an error.
///
/// UPSTREAM-BUG: whatever else was asked for is still written, as if no
/// record had been asked for.
#[test]
fn nothing_to_choose_automatically() {
    let unable = "Unable to automaticly select boot record for disk.img\n";
    let fat16 = Image::fixture("fat16");
    let mut device = fat16.device(Profile::FILE);
    assert_eq!(
        run_with(&mut device, "-w"),
        (1, String::new(), unable.to_owned())
    );
    assert_untouched(&device, &fat16, "-w");
    session(
        &Image::signed(),
        Profile::FILE,
        &[
            (
                "-p -w",
                1,
                "",
                &format!(
                    "{unable}Failed writing partition information to disk.img\n\
                     Device must be formatted as FAT16, FAT32, EXFAT or NTFS\n"
                ),
            ),
            (
                "-w -S cafe -f",
                1,
                "Windows Disk Signature 0x0000cafe successfully written to disk.img\n",
                unable,
            ),
        ],
    );
}

/// With nothing to write, the program names the file systems it finds and
/// describes the boot sector.
#[test]
fn file_systems_described() {
    session(
        &Image::new("empty", Vec::new()),
        Profile::FILE,
        &[("", 0, "disk.img has no x86 boot sector\n", "")],
    );
    session(
        &Image::fixture("fat16").with("unsigned", 0x1fe, &[0, 0]),
        Profile::FILE,
        &[(
            "",
            0,
            "disk.img has a FAT16 file system.\n\
             disk.img has no x86 boot sector\n",
            "",
        )],
    );
    session(
        &Image::fixture("ntfs"),
        Profile::FILE,
        &[(
            "",
            0,
            "disk.img has a NTFS file system.\n\
             disk.img has an x86 boot sector,\n\
             it is an unknown boot record\n\
             The OEM ID is NTFS    \n",
            "",
        )],
    );
    // An OEM ID that is not printable is left out.
    session(
        &Image::fixture("fat12").with("oem", 0x3, b"caf\xc3\xa9   "),
        Profile::FILE,
        &[(
            "",
            0,
            "disk.img has a FAT12 file system.\n\
             disk.img has an x86 boot sector,\n\
             it is an unknown boot record\n",
            "",
        )],
    );
    session(
        &Image::fixture("fat12"),
        Profile::DISK,
        &[
            (
                "-3 -f",
                0,
                "FAT32 DOS boot record successfully written to disk.img\n",
                "",
            ),
            (
                "",
                0,
                "disk.img has a FAT12 file system.\n\
                 disk.img has a FAT32 file system.\n\
                 disk.img has an x86 boot sector,\n\
                 it is exactly the kind of FAT32 DOS boot record this program\n\
                 would create with the switch -3 on a FAT32 partition.\n\
                 The OEM ID is MSWIN4.1\n",
                "",
            ),
        ],
    );
}

/// A boot record this program writes is described by the switch that
/// writes it.
#[test]
fn own_boot_records_described() {
    session(
        &Image::fixture("fat12"),
        Profile::FLOPPY,
        &[
            (
                "-1",
                0,
                "FAT12 boot record successfully written to disk.img\n",
                "",
            ),
            (
                "",
                0,
                "disk.img has a FAT12 file system.\n\
                 disk.img has an x86 boot sector,\n\
                 it is exactly the kind of FAT12 boot record this program\n\
                 would create with the switch -1 on a floppy.\n\
                 The OEM ID is MSWIN4.1\n",
                "",
            ),
        ],
    );
    session(
        &Image::fixture("exfat"),
        Profile::PARTITION,
        &[
            (
                "-x",
                0,
                "EXFAT NT6.0 boot record successfully written to disk.img\n\
                 EXFAT VBR checksum successfully written to disk.img\n",
                "",
            ),
            (
                "",
                0,
                "disk.img has a EXFAT file system.\n\
                 disk.img has an x86 boot sector,\n\
                 it is exactly the kind of EXFAT NT6.0 boot record this program\n\
                 would create with the switch -x on a EXFAT partition.\n\
                 The OEM ID is EXFAT   \n",
                "",
            ),
        ],
    );
    session(
        &Image::zeros(),
        Profile::DISK,
        &[
            (
                "-7",
                0,
                "Windows 7 master boot record successfully written to disk.img\n",
                "",
            ),
            (
                "-S cafe",
                0,
                "Windows Disk Signature 0x0000cafe successfully written to disk.img\n",
                "",
            ),
            (
                "",
                0,
                "disk.img has an x86 boot sector,\n\
                 it is a Microsoft 7 master boot record, like the one this\n\
                 program creates with the switch -7 on a hard disk device.\n\
                 It has windows disk signature 0x0000cafe.\n",
                "",
            ),
            (
                "-z",
                0,
                "Empty (zeroed) master boot record successfully written to disk.img\n",
                "",
            ),
            (
                "",
                0,
                "disk.img has an x86 boot sector,\n\
                 it is a zeroed non-bootable master boot record, like the one this\n\
                 program creates with the switch -z on a hard disk device.\n",
                "",
            ),
        ],
    );
}

/// A zeroed master boot record with a disk signature is described with
/// what its copy protection bytes mean.
#[test]
fn nearly_zeroed_master_boot_records_described() {
    let signed = Image::signed();
    for (image, signature, bytes) in [
        (
            signed.with("disk-signature", 0x1b8, &[0xfe, 0xca, 0, 0]),
            "0x0000cafe",
            "0x0000 (not copy protected)",
        ),
        (
            signed.with(
                "disk-signature",
                0x1b8,
                &[0x78, 0x56, 0x34, 0x12, 0x5a, 0x5a],
            ),
            "0x12345678",
            "0x5a5a (copy protected)",
        ),
        (
            signed.with("copy-protect", 0x1bc, &[0x01, 0x02]),
            "0x00000000",
            "0x0201 (unknown value)",
        ),
    ] {
        session(
            &image,
            Profile::FILE,
            &[(
                "",
                0,
                &format!(
                    "disk.img has an x86 boot sector,\n\
                     it is a non-bootable master boot record, almost like the one this\n\
                     program creates with the switch -z on a hard disk device but\n\
                     with windows disk signature {signature}\n\
                     which this program can write with switch -S and copy protect\n\
                     bytes {bytes}.\n"
                ),
                "",
            )],
        );
    }
}

/// Boot records this program does not write but has a name for.
#[test]
fn foreign_boot_records_described() {
    session(
        &Image::fixture("fat16").with("mswin", 0x3, b"MSWIN4.1"),
        Profile::FILE,
        &[(
            "",
            0,
            "disk.img has a FAT16 file system.\n\
             disk.img has an x86 boot sector,\n\
             it seems to be a FAT16 or FAT32 boot record, but it\n\
             differs from what this program would create with the\n\
             switch -6, -2, -8, -e or -3 on a FAT16 or FAT32 partition.\n\
             The OEM ID is MSWIN4.1\n",
            "",
        )],
    );
    session(
        &Image::fixture("exfat"),
        Profile::FILE,
        &[(
            "",
            0,
            "disk.img has a EXFAT file system.\n\
             disk.img has an x86 boot sector,\n\
             it seems to be a EXFAT boot record, but it differs from\n\
             what this program would create with the switch -x on a\n\
             EXFAT partition.\n\
             The OEM ID is EXFAT   \n",
            "",
        )],
    );
    session(
        &Image::signed().with("lilo", 0x6, b"LILO"),
        Profile::FILE,
        &[(
            "",
            0,
            "disk.img has an x86 boot sector,\n\
             it seems to be a LILO boot record, please use lilo to\n\
             create such boot records.\n",
            "",
        )],
    );
    session(
        &Image::signed().with("dos-f2", 0x0, super::super::payloads::MBR_DOS_F2),
        Profile::FILE,
        &[(
            "",
            0,
            "disk.img has an x86 boot sector,\n\
             it is a Microsoft DOS/NT/95A master boot record with the undocumented\n\
             F2 instruction. You will get equal functionality with the MBR this\n\
             program creates with the switch -d on a hard disk device.\n\
             It has windows disk signature 0x00000000.\n",
            "",
        )],
    );
}

/// The help is clap's: on stdout with status 0, naming the program as it
/// was called and listing every switch once.
#[test]
fn help() {
    let (status, out, err) = run(&words(&["--help"]));
    let help = String::from_utf8(out).unwrap();
    assert_eq!((status, err), (0, Vec::new()));
    assert!(
        help.contains("\nUsage: ms-sys [OPTIONS] <DEVICE>\n"),
        "{help}"
    );
    assert!(
        help.ends_with(
            "\nWarning: Writing the wrong kind of boot record to a device might\n\
             destroy partition information or file system!\n"
        ),
        "{help}"
    );
    let switches = RECORDS
        .iter()
        .map(|record| (record.short, record.long))
        .chain([
            ('w', "write"),
            ('f', "force"),
            ('l', "wipelabel"),
            ('p', "partition"),
            ('H', "heads <N>"),
            ('B', "bps <N>"),
            ('O', "writeoem <ID>"),
            ('S', "writewds <HEX>"),
            ('h', "help"),
            ('V', "version"),
        ]);
    for (short, long) in switches {
        let entry = format!("\n  -{short}, --{long} ");
        assert_eq!(help.matches(&entry).count(), 1, "{entry:?}");
    }

    // It wins over everything else on the command line, the device included.
    for arguments in [
        &["-h"][..],
        &["-m", "-1", "-h"],
        &["--help", "-Z", "x", "y"],
    ] {
        assert_eq!(
            run(&words(arguments)),
            (0, help.clone().into_bytes(), Vec::new()),
            "{arguments:?}"
        );
    }
}

/// The version is the port's own name and version, on stdout with status
/// 0, and the device is not gone near.
#[test]
fn version_switch_reports_the_port() {
    let prefix = format!("ms-sys {}", env!("CARGO_PKG_VERSION"));
    for arguments in [
        &["-V"][..],
        &["--version"],
        &["-V", "no-such-device"],
        &["-m", "-f", "--version"],
        &["--version", "-m", "-f", "no-such-device"],
    ] {
        let (status, out, err) = run(&words(arguments));
        let out = String::from_utf8(out).unwrap();
        assert_eq!((status, err), (0, Vec::new()), "{arguments:?}");
        assert!(
            out.starts_with(&prefix) && out.ends_with('\n') && out.lines().count() == 1,
            "{arguments:?}: {out:?}"
        );
    }
}

// Real files.

/// Runs the program with `switches` on `device` and returns its exit status
/// and what it printed, with the device's path taken out of it.
fn run_on(device: &Path, switches: &[&str]) -> Printed {
    let mut arguments = words(switches);
    arguments.push(device.into());
    let (status, out, err) = run(&arguments);
    let named = |bytes| {
        String::from_utf8(bytes)
            .unwrap()
            .replace(device.to_str().unwrap(), "DEVICE")
    };
    (status, named(out), named(err))
}

/// An exit status, with what went to stdout and to stderr.
type Printed = (i32, String, String);

fn printed(text: &str) -> Printed {
    (0, text.to_owned(), String::new())
}

fn failed(text: &str) -> Printed {
    (1, String::new(), text.to_owned())
}

/// Devices that cannot be opened, or only for reading, and one that can: a
/// file is read and written like the images the other tests keep in memory.
#[test]
fn devices_are_opened_by_path() {
    let scratch = tempfile::tempdir().unwrap();
    let directory = scratch.path().to_path_buf();
    let image = Image::fixture("fat16");
    let device = directory.join(DEVICE);
    fs::write(&device, &image.bytes).unwrap();
    let inside = device.join("inside");
    let missing = directory.join("missing.img");

    let diagnosis = "DEVICE has a FAT16 file system.\nDEVICE has an x86 boot sector,\n\
                     it is an unknown boot record\nThe OEM ID is mkfs.fat\n";
    let absent = failed("Unable to open DEVICE, No such file or directory\n");
    for switches in [&[][..], &["-m"], &["-m", "-f"]] {
        assert_eq!(run_on(&missing, switches), absent, "{switches:?}");
    }
    for switches in [&[][..], &["-p"]] {
        assert_eq!(
            run_on(&inside, switches),
            failed("Unable to open DEVICE, Not a directory\n"),
            "{switches:?}"
        );
    }
    // A directory opens for reading, and nothing can be read from it.
    assert_eq!(
        run_on(&directory, &[]),
        printed("DEVICE has no x86 boot sector\n")
    );
    for switches in [&["-m", "-f"][..], &["-O", "x"]] {
        assert_eq!(
            run_on(&directory, switches),
            failed("Unable to open DEVICE, Is a directory\n"),
            "{switches:?}"
        );
    }

    // A regular file answers none of the device queries.
    assert_eq!(run_on(&device, &[]), printed(diagnosis));
    assert_eq!(
        run_on(&device, &["-w"]),
        failed("Unable to automaticly select boot record for DEVICE\n")
    );
    assert_eq!(
        run_on(&device, &["-p"]),
        failed(
            "Failed writing start sector to DEVICE, this is only possible to do with\n\
             real partitions!\n"
        )
    );
    assert_eq!(
        run_on(&device, &["-O", "x"]),
        failed(
            "DEVICE does not seem to be a floppy or disk partition device,\n\
             use the switch -f to force writing of OEM ID\n"
        )
    );
    assert_eq!(
        run_on(&device, &["-S", "1"]),
        failed(
            "DEVICE does not seem to be a disk device,\n\
             use the switch -f to force writing of a master boot record\n"
        )
    );
    assert_image(&fs::read(&device).unwrap(), &image.bytes, "refused");

    // Read-only, unless permissions do not stop this user.
    fs::set_permissions(&device, fs::Permissions::from_mode(0o444)).unwrap();
    if fs::OpenOptions::new().write(true).open(&device).is_err() {
        assert_eq!(run_on(&device, &[]), printed(diagnosis));
        for switches in [
            &["-m", "-f"][..],
            &["-w"],
            &["-p"],
            &["-O", "x"],
            &["-S", "1"],
        ] {
            assert_eq!(
                run_on(&device, switches),
                failed("Unable to open DEVICE, Permission denied\n"),
                "{switches:?}"
            );
        }
        assert_image(&fs::read(&device).unwrap(), &image.bytes, "read-only");
    }
    fs::set_permissions(&device, fs::Permissions::from_mode(0o644)).unwrap();

    // What the same call writes to this image in memory.
    assert_eq!(
        run_on(&device, &["-m", "-f"]),
        printed("Windows 2000/XP/2003 master boot record successfully written to DEVICE\n")
    );
    assert_image(
        &fs::read(&device).unwrap(),
        &patched(&image.bytes, &[(0x0, MBR_2000)]),
        "written",
    );

    // With the file gone, so is everything under it.
    fs::remove_file(&device).unwrap();
    for (path, switches) in [(&device, &["-w"][..]), (&inside, &["-p"])] {
        assert_eq!(run_on(path, switches), absent, "{switches:?}");
    }

    // A name that is not valid UTF-8 is printed as it is.
    let odd = directory.join(raw(b"missing-\xff.img"));
    let mut expected = b"Unable to open ".to_vec();
    expected.extend_from_slice(odd.as_os_str().as_bytes());
    expected.extend_from_slice(b", No such file or directory\n");
    assert_eq!(run(&[odd.clone().into()]), (1, Vec::new(), expected));
}
