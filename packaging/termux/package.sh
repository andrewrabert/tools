#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "usage: $0 <bertbox-binary> <version> <output-directory>" >&2
    exit 2
fi

binary=$(realpath "$1")
version=$2
output=$(realpath -m "$3")
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

if [[ ! -x "$binary" ]]; then
    echo "not an executable file: $binary" >&2
    exit 2
fi
if ! dpkg --validate-version "$version"; then
    echo "invalid Debian package version: $version" >&2
    exit 2
fi

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
root="$stage/root"
mkdir -p "$root/DEBIAN" "$root/data/data/com.termux/files/usr/bin" \
    "$root/data/data/com.termux/files/usr/share/doc/bertbox" "$output"

install -m 0755 "$binary" "$root/data/data/com.termux/files/usr/bin/bertbox"
install -m 0644 "$repo/LICENSE" \
    "$root/data/data/com.termux/files/usr/share/doc/bertbox/copyright"

installed_size=$(du -sk "$root/data" | cut -f1)
cat > "$root/DEBIAN/control" <<EOF
Package: bertbox
Version: $version
Architecture: aarch64
Maintainer: Andrew Rabert <ar@nullsum.net>
Installed-Size: $installed_size
Section: utils
Priority: optional
Homepage: https://github.com/andrewrabert/bertbox
Description: A tree of Markdown notes and tasks
 bertbox exposes one set of note, log, and task operations through a CLI,
 an HTTP API, and MCP.
EOF

package="$output/bertbox_${version}_aarch64.deb"
dpkg-deb --root-owner-group --build "$root" "$package"
printf '%s\n' "$package"
