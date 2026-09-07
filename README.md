# shr-rec

A standalone multichannel recorder/player for a Raspberry Pi and a **40×13**
touch terminal. Runs directly or as gadget `rec` from a launcher.

**Current status: foundation, not yet an audio recorder/player.** The executable
provides input slots, session-only arming, private configuration, touch/keyboard
controls and clean Exit. Recording, playback, meters and MIDI are not connected.

```sh
cargo build --locked --release
mkdir -p "$HOME/.local/bin"
ln -s "$PWD/target/release/shr-rec" "$HOME/.local/bin/shr-rec"
shr-rec --help
shr-rec
```

The one-time link assumes `~/.local/bin` is on PATH and follows subsequent
release builds in this checkout. It refuses to replace an existing command.

Rust is pinned to **1.97.1**. No JACK server, audio client, MIDI connection,
service or launcher is started by this version. Existing terminal font and
console settings remain unchanged after exit.

Touch PREV/NEXT to select, ARM to toggle and EXIT to return to the caller.
Keyboard equivalents: arrows, Space, `s` for Stop, `q` or Escape for Exit.
RECORD and PLAY currently explain that the engine is not implemented.

- [Implementation plan](docs/PLAN.md)
- [Current behavior and validation](docs/STATUS.md)
- [Configuration and launcher boundary](docs/LOCAL_SETUP.md)
- [Architecture and remaining engine contracts](docs/ARCHITECTURE.md)
- [Reference provenance](THIRD_PARTY.md)

Run normal software checks with `cargo test --locked --all-targets`; Python 3
and `mkfifo` are needed for isolated CLI/terminal regressions. Tests never open
JACK or MIDI. See the status document for acceptance limits.

MIT licensed; see [LICENSE](LICENSE).
