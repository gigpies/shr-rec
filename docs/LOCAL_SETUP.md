# Local setup and launcher boundary

Build with `cargo build --locked --release`. The exact toolchain is selected by
`rust-toolchain.toml`; install it with
`rustup toolchain install 1.97.1 --profile minimal --component rustfmt,clippy`
if absent. The tested platform is AArch64 Linux. The first foundation needs no
JACK/ALSA development packages because no audio adapter is included yet.

Run `target/release/rec` in a terminal. The UI uses the upper-left 40×13 cells:
body rows 1–10, contextual controls 11–12, shared status row 13. Smaller
terminals show a size notice and a touch Exit target on the last row.

## Private data

Root selection, in order:

1. `--data-root PATH`
2. `REC_DATA_ROOT`
3. `$XDG_DATA_HOME/rec` (XDG_DATA_HOME must be absolute)
4. `$HOME/.local/share/rec`

`rec --data-root ./user --init-config` creates 18 disarmed, unassigned inputs
in `config.json`, using file mode 0600 and new-directory mode 0700 (subject to
umask). It refuses to replace an existing file. Existing parent directory
permissions are left intact. Ordinary startup reads only; a missing file uses
inert defaults without creating the root. Malformed, newer-version,
over-limit, nonregular or final-component symlink configs fail before entering
raw terminal mode. A user-selected root is trusted; it can itself be a symlink.

Configuration format 1 currently contains `format_version` and `tracks`.
Each track has a unique ASCII `id`, printable `label`, `armed` boolean and
nullable `preferred_source`. Sources are remembered exact strings: no substring,
channel-number or first-port fallback. Refresh resolution does not rewrite them.
The foundation has no port discovery, so remembered sources show MISSING.
There are at most 64 tracks; 18 is the future Pi acceptance target.

This is an initial **configuration** schema, not the future take manifest.
Stereo groups, output routes and controller mappings are still pending. Arm
changes in the current shell last only for that process; edit the private file
while stopped to set startup preferences. Do not track real routes or takes.

## Direct and launcher invocation

The stable gadget ID and executable are `rec`. The caller launches an ordinary
process, for example `rec --data-root /private/path`, and waits for its exit.
There is no launcher SDK, daemon, socket, shared crate or network dependency.
`--help` and `--version` work without HOME, audio or an interactive terminal.
Errors exit nonzero and print to stderr after terminal cleanup.

Touch EXIT, `q`, Escape and Ctrl-C return to the caller. SIGTERM, SIGINT and
SIGHUP request graceful shell exit. The terminal guard restores raw mode,
cursor visibility, mouse reporting and alternate screen on normal exit,
handled errors and unwinding. SIGKILL, power loss and abort cannot run cleanup.

The engine-independent state contract already requires Stop and finalize /
Cancel during recording, waits for producer/worker completion and keeps a
damaged result visible before another deliberate Exit. No engine is wired to
that protocol yet. Before recording is enabled, the signal path must drive its
quiescence/finalization effects and leave crash-recoverable files; current
terminal tests establish only shell cleanup.
