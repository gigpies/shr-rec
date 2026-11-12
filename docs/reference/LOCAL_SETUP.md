# Local setup and launcher boundary

Detailed reference. For everyday use, start with the [operating guide](../LOCAL_SETUP.md).
[Engineering](../ENGINEERING.md).


Build with `cargo build --locked --release`. The exact toolchain is selected by
`rust-toolchain.toml`; install it with
`rustup toolchain install 1.97.1 --profile minimal --component rustfmt,clippy`
if absent. The tested platform is AArch64 Linux. Capture and playback load system
`libjack.so.0` dynamically and needs an existing JACK server when explicitly discovering destinations or starting audio. No JACK
Rust dependency or JACK/ALSA development package is needed to build.

From the checkout, install a user-local command once:

```sh
mkdir -p "$HOME/.local/bin"
ln -s "$PWD/target/release/shr-rec" "$HOME/.local/bin/shr-rec"
shr-rec --version
shr-rec
```

`~/.local/bin` must be on PATH (it already is on the development machine).
The link follows subsequent release builds and refuses to overwrite an existing
command. Keep the checkout at this path, or recreate the link after moving it.
This installs no service and does not start audio. To remove the command,
remove only the `~/.local/bin/shr-rec` symlink.

Run `shr-rec` in a terminal. The UI uses the upper-left 40×13 cells:
body rows 1–10, contextual controls 11–12, shared status row 13. Smaller
terminals show a size notice and a touch Exit target on the last row.

## Private data

Root selection, in order:

1. `--data-root PATH`
2. `REC_DATA_ROOT`
3. `$XDG_DATA_HOME/rec` (XDG_DATA_HOME must be absolute)
4. `$HOME/.local/share/rec`

`shr-rec --data-root ./user --init-config` creates 18 disarmed, unassigned inputs
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
SETUP → REFRESH discovers current exact sources without changing preferences.
There are at most 64 tracks; 18 remains the separate sustained Pi acceptance target.

The standalone capture adapter keeps explicit SAVE preferences in private
`capture.json`: version, destination and the same ordered track schema. It does
not rewrite initial config.json. Playback output routes/mix remain separate in
`playback.json`; controller mappings remain pending. Ordinary startup is read-only.
Capture defaults to DATA_ROOT/takes or its remembered destination; an explicit
`--takes-root` takes precedence for this run. DEST can select an existing directory
without a keyboard. No real routes or takes belong in tracked files.
[Capture format and lifecycle](../CAPTURE.md).

## Direct and launcher invocation

The stable gadget ID is `rec`; the executable is `shr-rec`. The caller launches an ordinary
process, for example `shr-rec --data-root /private/path`, and waits for its exit.
There is no launcher SDK, daemon, socket, shared crate or network dependency.
`--help` and `--version` work without HOME, audio or an interactive terminal.
Errors exit nonzero and print to stderr after terminal cleanup.

Touch EXIT, `q` and Ctrl-C return to the caller. Escape cancels/goes back in
playback pages and exits from input setup. SIGTERM, SIGINT and SIGHUP request
graceful exit. Playback first closes its JACK client and joins its disk worker.
The terminal guard restores raw mode,
cursor visibility, mouse reporting and alternate screen on normal exit,
handled errors and unwinding. SIGKILL, power loss and abort cannot run cleanup.

Capture EXIT offers Stop and finalize / Cancel, waits for producer/worker
completion, and keeps a damaged result visible before another deliberate Exit.
Signals and terminal read/draw errors drive the same producer barrier and writer
completion; any retained result/path is printed after terminal restoration.
Finalization runs off the UI thread and can wait for in-flight filesystem I/O.
The raw C ABI remains host-owned and does not acquire devices. See
[capture](../CAPTURE.md) and [playback](../PLAYBACK.md) for exact routes,
format compatibility, failure behavior and offline acceptance limits.

## Executable naming

The project and command are `shr-rec`. Do not install a `rec` alias: SoX already
provides that recording command on other users' systems (see the
[SoX manual](https://manpages.debian.org/stretch/sox/rec.1.en.html)). A launcher catalog
uses ID `rec`, display name `SHR Rec`, and an explicit path to `shr-rec`; the ID
must never be used as an implicit executable fallback. The existing
`REC_DATA_ROOT` variable and `rec` data-directory suffix remain unchanged.
The source checkout directory name does not affect the executable name.

## Checkout name

The repository and checkout directory are `shr-rec`. The executable is
`shr-rec`; the launcher gadget ID remains `rec`. If moving an older checkout,
update its `~/.local/bin/shr-rec` link to the new checkout’s release binary.
