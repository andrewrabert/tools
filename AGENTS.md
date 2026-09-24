# AGENTS.md

## What this is

bertbox is a multicall binary (busybox-style) of personal command-line tools.
A tool runs as `bertbox <tool>`, `bertbox-<tool>`, or `<tool>`.
Code that runs bertbox executes `dispatch::program()` with `BERTBOX_AS_BERTBOX=1` in the environment and the tool name as the first argument.

## Development

Use or create `just` (`justfile`) recipes for _any_ repeatable development task.

### Organization
Starts the binary and dispatches to a tool.
- `src/main.rs` entry point & tool registry
- `src/dispatch.rs` running bertbox
- `src/env_file.rs` global repeatable `--env-file` flag

One module per tool.
- `src/tools/` tools
- `src/tools/external/` tools from upstream crates

Helpers that many tools share.
- `src/atomic_write.rs` atomic file writes
- `src/chardet.rs` character encoding detection & decoding
- `src/dirs.rs` user base directories (home, cache, config, runtime)
- `src/input.rs` input from stdin, args, or clipboard
- `src/mime.rs` MIME type detection
- `src/picker.rs` terminal picker
- `src/pool.rs` worker thread pool
- `src/source.rs` stdin or file input
- `src/urls.rs` URL parsing & extraction
- `src/walk.rs` directory walking

Wrappers for external programs and services.
- `src/clipboard.rs` clipboard copy & paste
- `src/git.rs` using git
- `src/http.rs` HTTP client
- `src/mkvmerge.rs` using mkvmerge
- `src/sqlite.rs` SQLite databases
- `src/tmux.rs` using tmux

### CLI
- When stdin is a TTY, a CLI takes its input only from its arguments.
