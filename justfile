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

[positional-arguments]
test *args:
    cargo test "$@"

# Regenerate the surface-spec fixture outputs from their Rust and Python inputs with the debug binary
surface-spec-fixtures: build
    #!/usr/bin/env bash
    set -euo pipefail
    bertbox="{{justfile_directory()}}/target/debug/bertbox"
    for language in rust python; do
        case "$language" in
            rust) extension=rs ;;
            python) extension=py ;;
        esac
        cd "{{justfile_directory()}}/src/tools/surface_spec/testdata/$language"
        for input in *."$extension"; do
            name="${input%."$extension"}"
            BERTBOX_AS_BERTBOX=1 "$bertbox" surface-spec capture "$input" | jq . > "$name.json"
            for section in additions deletions; do
                BERTBOX_AS_BERTBOX=1 "$bertbox" surface-spec render "$name.json" "--$section" \
                    --language "$language" > "$name.$section.md"
            done
        done
    done

# List outdated dependencies
outdated:
    @cargo outdated --manifest-path {{justfile_directory()}}/Cargo.toml --workspace --root-deps-only

[no-exit-message]
[positional-arguments]
run *args:
    cargo run -q -- "$@"

build-release:
    cargo build --release
