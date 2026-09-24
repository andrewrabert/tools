set default-list := true

build:
    cargo build

# Cross-compile the release binary for Termux/Android (arm64) via `cross`
build-android:
    #!/usr/bin/env sh
    set -eu
    cd "{{justfile_directory()}}"
    tools="$PWD/target/tools"
    cross="$tools/bin/cross"
    if [ ! -x "$cross" ]; then
        cargo install cross --locked --root "$tools"
    fi
    if [ -z "${CROSS_CONTAINER_ENGINE:-}" ] && command -v podman >/dev/null 2>&1; then
        export CROSS_CONTAINER_ENGINE=podman
    fi
    "$cross" build --release --target aarch64-linux-android
    echo "binary: $PWD/target/aarch64-linux-android/release/bertbox"

check:
    cargo check

[positional-arguments]
clippy *args:
    cargo clippy --all-targets "$@"

fmt:
    cargo fmt

[no-exit-message]
[positional-arguments]
run *args:
    cargo run -q -- "$@"

build-release:
    cargo build --release
