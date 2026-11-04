# bertbox

A multicall binary of personal command-line tools.
Run a tool as `bertbox <tool>`, `bertbox-<tool>`, or `<tool>`.

`bertbox install DIR` links each tool name in `DIR` to the binary.
The External group is linked only when selected with `--group` or `--tool`.

## Tools

### Archive

Create and unpack archives

- `archive`: Create archives in a variety of formats
- `extract`: Extract archives in a variety of formats

### Claude

- `claude-statusline`: Render the Claude Code status line from the hook json on stdin

### Clipboard

- `cbcopy`: Copy stdin to the clipboard
- `cbpaste`: Print the clipboard

### Encoding

- `b64d`: Decode nested base64 from arguments, stdin, or the clipboard
- `b64e`: Encode an argument, stdin, or the clipboard as base64
- `binary2hex`: Convert binary digits to hexadecimal
- `encode-utf8`: Reencode files in UTF-8
- `hex2binary`: Convert hexadecimal digits to binary
- `hexd`: Hex dump data in xxd format, or convert a hex dump back to binary
- `qr`: Print a QR code of an argument or stdin to the terminal
- `urldecode`: Percent-decode an argument or stdin
- `urlencode`: Percent-encode an argument or stdin
- `zlib`: Compress or decompress zlib data

### Files

- `3ds-rom-tool`: Decrypt, encrypt, or convert 3DS (NCSD/CCI) ROMs
- `broken-links`: List broken symlinks: the paths given, or those directly inside them
- `case-collisions`: List paths under a directory that would collide on a case-insensitive filesystem
- `code-nuke`: Delete Rust code in bulk: statements around a pattern, tests, or comments
- `empty-tree`: List the tops of empty directory trees
- `escape-files-windows`: Strip characters that are invalid on Windows from file paths
- `file-hash-recorder`: Record file hashes in a file_info.db and query them
- `find-dupes`: Find duplicate files by content
- `find-file-exts`: List the file extensions found under paths or named on stdin
- `mime`: Print the MIME type of files by their content
- `mime-ext`: Group files by detected MIME type and check or fix their extensions
- `mkv-preview`: Summarize the tracks of a Matroska file
- `multihash`: Print the hashes of a file, or of every file under a directory, as JSON
- `path-renamer`: Rename sibling paths by editing their names in nvim; paths are read one per line
- `pathbin`: List executables in path
- `pathcacher`: Cache a command's stdout against a path's size and mtime
- `preview`: Preview files with syntax highlighting and metadata extraction
- `rerename`: Rename files using regex substitution
- `surface-spec`: Render the declarations in Rust and Python source files as a surface spec, or as the additions and deletions of a change
- `syncthing-conflicts`: List Syncthing conflict files
- `syncthing-stversions`: List, prune, and restore Syncthing's .stversions files

### Git

- `git-auto-update`: Merge upstream, commit every change, and push
- `git-check-tag-is-pushed`: Check that a tag and every commit it holds are pushed
- `git-find-repos`: List every git repository under a directory
- `git-remote-tags`: List the tags of a remote repository in version order
- `git-retime`: Rewrite the author and committer date of commits
- `git-s`: Summarize remotes, branches, stashes, and changes
- `git-sync`: Clone, fetch, and track source repositories under ~/src
- `git-tag-timestamp`: List tags with their creation dates
- `git-undelete`: Restore files deleted from a git repository
- `git-web`: Print and open the web page of a repository or a file in it

### JSON

- `json-flat`: Flatten nested json objects into delimiter-joined keys
- `json-nest`: Nest a flat json object by splitting its keys on a delimiter
- `json-re`: Filter a json object of strings by key and value patterns

### Just

- `lust`: Run just against a personal per-project justfile stored outside the project
- `lust-manage`: Inspect the personal justfiles lust runs

### Media

- `audio-tag`: Dump and restore music tags to/from JSON
- `cue-tool`: Check, format, print, and list cue sheets
- `img-compare`: Print a hash of each image's decoded pixels
- `img-convert`: Convert or resize images
- `img-fixcolorspace`: Recode PNGs whose pixels are gray but stored as color
- `img-metadata`: Copy metadata between images
- `img-optim`: Losslessly shrink images in place
- `img-size`: Print the width and height of images
- `mkv-convert`: Merge videos and their sidecar subtitles into Matroska files
- `mkv-extract-languages`: Extract the English and undetermined tracks of a Matroska file
- `mkv-find-lang`: List Matroska files by the languages of their audio and subtitle tracks
- `mkv-info-recorder`: Record Matroska track info in a mkv_info.db and list files with unexpected languages
- `mkv-language-strip`: Remove audio and subtitle tracks in other languages from Matroska files
- `mkv-set-title-from-filename`: Set the title of Matroska files to their file name
- `music-organizer`: Organize a music library by its metadata
- `reencode-lossless`: Reencode lossless audio files as FLAC or WAV

### Network

- `dl`: Download urls from arguments, stdin, or the clipboard
- `forgejo`: Manage repositories on a Forgejo server
- `geoip`: Look up the geolocation of addresses from arguments or stdin
- `immich-remote`: Manage trash and libraries on an Immich server
- `jdownloader-remote`: Control JDownloader through its local API
- `mac-address`: Print a random locally administered MAC address
- `network-scan`: List the live hosts of a network with their hostnames
- `publicipv4`: Print the public IPv4 address
- `publicipv6`: Print the public IPv6 address

### System

- `cdemu-tool`: Load disc images into CDEmu daemons run as systemd user units
- `exit-code`: Run a command, then print its exit status and the time
- `fanpipe`: Fan messages out to subscribers over named pipes
- `lmk`: Rerun a command whenever lmk is run without one
- `open`: Open files or URLs with the desktop opener, detached from the terminal
- `pending-sync`: Print how much data is waiting to be written to disk
- `ud`: Mount disk images, or pick a device, disc image, or sshfs mount to (un)mount

### Tmux

- `tmux-attach`: Attach to the first tmux session, detaching other clients, or start one
- `tmux-attach-ssh`: SSH to a host and attach to its tmux session, in a new tmux window when in tmux
- `tmux-chooser`: Pick a tmux window or labeled pane and switch to it
- `tmux-pane-title`: Set or clear the label of a tmux pane
- `tmux-ssh`: Pick an SSH host and attach to its tmux session in this window

### Text

- `minijinja`: Render a Jinja template
- `txt-alternating`: aLtErNaTiNg cAsE arguments, stdin, or the clipboard, copied to the clipboard
- `txt-line-end`: Rewrite the line endings of files in place
- `txt-lower`: Lowercase an argument or stdin
- `txt-trim`: Strip surrounding whitespace from stdin
- `txt-trimlines`: Strip each line of stdin
- `txt-upper`: Uppercase an argument or stdin
- `txt-upper-first`: Title Case arguments, stdin, or the clipboard, copied to the clipboard
- `urlextract`: Print the unique URLs found in a file or stdin

### External

Commands from upstream crates

- `ast-grep`: Search and rewrite code by syntax (upstream ast-grep)
- `dotenvy`: Run a command using the environment in a .env file (upstream dotenvy)
- `qrencode`: Print text as a QR code to the terminal (upstream qrcode)
- `uncomment`: Remove comments from source files (upstream uncomment)

### Bertbox

Set up and inspect bertbox itself

- `completions`: Generate shell completions
- `install`: Link tool names to this binary
- `list`: List every tool
