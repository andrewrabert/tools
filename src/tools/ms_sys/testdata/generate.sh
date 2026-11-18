#!/bin/sh
# Regenerates the file system fixtures in this directory.
#
# Each fixture is the first 128 KiB of a freshly formatted image: ms-sys only
# reads and writes boot sectors, and the highest offset it touches (the exFAT
# backup checksum with 4096-byte sectors) is below that.
#
# Needs dosfstools, exfatprogs and ntfs-3g. The fixtures are committed, so
# this only has to be run to change them; output is not byte-reproducible
# for exFAT and NTFS, which embed random serial numbers.
set -eu
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

make_fixture() {
    name=$1 size=$2
    shift 2
    truncate -s "$size" "$work/$name"
    "$@" "$work/$name" >"$work/log" 2>&1 || {
        cat "$work/log" >&2
        exit 1
    }
    head -c 131072 "$work/$name" >"$name.img"
}

make_fixture fat12 1440K mkfs.fat -F 12 -n FIXTURE12 -i 12345678 --invariant
make_fixture fat16 32M mkfs.fat -F 16 -n FIXTURE16 -i 12345678 --invariant
make_fixture fat32 64M mkfs.fat -F 32 -n FIXTURE32 -i 12345678 --invariant
make_fixture exfat 64M mkfs.exfat -L FIXTUREEX
make_fixture ntfs 16M mkfs.ntfs -F -f -q -L FIXTURENT
