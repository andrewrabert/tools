# AGENTS.md

## What this is

bertbox is a multicall binary (busybox-style) of personal command-line tools.
A tool runs as `bertbox <tool>`, `bertbox-<tool>`, or `<tool>`.
Code that runs bertbox executes `dispatch::program()` with `BERTBOX_AS_BERTBOX=1` in the environment and the tool name as the first argument.

## Development

Use or create `just` (`justfile`) recipes for _any_ repeatable development task.

Before commiting changes, ensure the `fmt`, `clippy`, `test`, and `build` recipes all pass.

### Organization
The repository is a virtual Cargo workspace; every crate lives under `crates/<name>/` as package `bertbox-<name>`.
- `crates/bertbox/` the bertbox binary (package `bertbox`)

Library crates for external services, free of CLI concerns (no clap, terminal output, or anyhow).
- `crates/http/` HTTP plumbing shared by bertbox and the API clients
- `crates/forgejo/` Forgejo API client
- `crates/immich/` Immich API client
- `crates/jdownloader/` JDownloader API client

Starts the binary and dispatches to a tool.
- `crates/bertbox/src/main.rs` entry point & tool registry
- `crates/bertbox/src/dispatch.rs` running bertbox
- `crates/bertbox/src/env_file.rs` global repeatable `--env-file` flag

One module per tool.
- `crates/bertbox/src/tools/` tools
- `crates/bertbox/src/tools/external/` tools from upstream crates

Helpers that many tools share.
- `crates/bertbox/src/atomic_write.rs` atomic file writes
- `crates/bertbox/src/chardet.rs` character encoding detection & decoding
- `crates/bertbox/src/dirs.rs` user base directories (home, cache, config, runtime)
- `crates/bertbox/src/input.rs` input from stdin, args, or clipboard
- `crates/bertbox/src/install_target.rs` install targets (system, arch, package manager) & the host's own
- `crates/bertbox/src/mime.rs` MIME type detection
- `crates/bertbox/src/picker.rs` terminal picker
- `crates/bertbox/src/pool.rs` worker thread pool
- `crates/bertbox/src/source.rs` stdin or file input
- `crates/bertbox/src/urls.rs` URL parsing & extraction
- `crates/bertbox/src/walk.rs` directory walking
- `crates/bertbox/src/yaml.rs` YAML read into JSON values & written in block style

Wrappers for external programs and services.
- `crates/bertbox/src/clipboard.rs` clipboard copy & paste
- `crates/bertbox/src/ffmpeg.rs` using ffmpeg & ffprobe
- `crates/bertbox/src/git.rs` using git
- `crates/bertbox/src/http.rs` HTTP client builder with bertbox's user agent
- `crates/bertbox/src/mkvmerge.rs` using mkvmerge
- `crates/bertbox/src/sqlite.rs` SQLite databases
- `crates/bertbox/src/tmux.rs` using tmux
- `crates/bertbox/src/wayland.rs` the Wayland session

### CLI
- When stdin is a TTY, a CLI takes its input only from its arguments.
