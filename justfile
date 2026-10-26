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

# Regenerate the surface-spec fixture outputs from their Rust and Python sources and changes with the debug binary
surface-spec-fixtures: build
    #!/usr/bin/env bash
    set -euo pipefail
    bertbox="{{justfile_directory()}}/target/debug/bertbox"
    scratch=$(mktemp -d)
    trap 'rm -rf "$scratch"' EXIT
    for language in rust python; do
        case "$language" in
            rust) extension=rs ;;
            python) extension=py ;;
        esac
        for fixture in "{{justfile_directory()}}/src/tools/surface_spec/testdata/$language"/*/; do
            # An error fixture records the message the source fails with, which is written by hand.
            [[ -e "$fixture/error" ]] && continue
            name=$(basename "$fixture")
            # A change fixture records the spec of `head` since `base`, committed in a scratch repository.
            if [[ -e "$fixture/base" ]]; then
                repo="$scratch/$name"
                git init -q "$repo"
                cp "$fixture/base" "$repo/$name.$extension"
                git -C "$repo" add "$name.$extension"
                git -C "$repo" -c user.name=fixture -c user.email=fixture -c commit.gpgsign=false \
                    commit -q --no-verify -m base
                cp "$fixture/head" "$repo/$name.$extension"
                (cd "$repo" && BERTBOX_AS_BERTBOX=1 "$bertbox" surface-spec --git HEAD \
                    --language "$language" "$name.$extension") > "$fixture/change"
                continue
            fi
            # The capture records the source as `<name>.<extension>`, the path the tests parse it as.
            cp "$fixture/source" "$scratch/$name.$extension"
            (cd "$scratch" && BERTBOX_AS_BERTBOX=1 "$bertbox" surface-spec --json "$name.$extension") \
                | jq . > "$scratch/$name.json"
            cp "$scratch/$name.json" "$fixture/capture"
            for spec in additions deletions; do
                BERTBOX_AS_BERTBOX=1 "$bertbox" surface-spec --input-json "--$spec" "$scratch/$name.json" \
                    --language "$language" > "$fixture/$spec"
            done
            BERTBOX_AS_BERTBOX=1 "$bertbox" surface-spec --input-json "$scratch/$name.json" \
                --language "$language" > "$fixture/surface-spec"
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
