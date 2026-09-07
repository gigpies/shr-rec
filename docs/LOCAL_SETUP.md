# Local setup and launcher boundary

Build with `cargo build --locked --release`. The exact toolchain is selected by
`rust-toolchain.toml`; install it with
`rustup toolchain install 1.97.1 --profile minimal --component rustfmt,clippy`
if absent. The tested platform is AArch64 Linux. The first foundation needs no
JACK/ALSA development packages because no audio adapter is included yet.

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
The foundation has no port discovery, so remembered sources show MISSING.
There are at most 64 tracks; 18 is the future Pi acceptance target.

This is an initial **configuration** schema, not the future take manifest.
Stereo groups, output routes and controller mappings are still pending. Arm
changes in the current shell last only for that process; edit the private file
while stopped to set startup preferences. Do not track real routes or takes.

## Direct and launcher invocation

The stable gadget ID is `rec`; the executable is `shr-rec`. The caller launches an ordinary
process, for example `shr-rec --data-root /private/path`, and waits for its exit.
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

## Executable naming

The project and command are `shr-rec`. Do not install a `rec` alias: SoX already
provides that recording command on other users' systems (see the
[SoX manual](https://manpages.debian.org/stretch/sox/rec.1.en.html)). A launcher catalog
uses ID `rec`, display name `SHR Rec`, and an explicit path to `shr-rec`; the ID
must never be used as an implicit executable fallback. The existing
`REC_DATA_ROOT` variable and `rec` data-directory suffix remain unchanged.
The source checkout directory name does not affect the executable name.
