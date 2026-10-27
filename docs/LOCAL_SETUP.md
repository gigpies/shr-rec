# Current use and setup

[Project home](../README.md) · [Engineering](ENGINEERING.md)

**The standalone application cannot record or play yet.** It offers input slots,
session-only arming and configuration. The recording library is used by GigPies;
that integration does not make the shell's Record button operational.

## Open the shell

Use this checkout's pinned Rust toolchain:

```sh
cargo build --locked --release
./target/release/shr-rec
```

Use a terminal of at least **40×13** cells. Touch PREV/NEXT, ARM and EXIT, or use
arrows, Space and q/Escape. Arming lasts only for this run. No audio device opens.

## Keep settings separate

Default data location is `$XDG_DATA_HOME/rec`, or `~/.local/share/rec`.
Use `--data-root /your/private/folder` for another location.

```sh
./target/release/shr-rec --data-root ./user --init-config
```

This creates a new configuration with 18 disarmed, unassigned inputs and refuses
to replace an existing file. The shell has no port discovery, so remembered
sources appear MISSING. Record and Play explain the unfinished engine connection.

## Recording through GigPies

The integrated recorder writes aligned mono PCM24 stems and a take manifest.
Desk recording controls remain unavailable; a configured engine session is
required. [GigPies recording scope](https://github.com/gigpies/gigpies/blob/main/docs/README.md#recording).

A damaged or interrupted take is not a completed take. Recovery details and the
file format belong in the [recording reference](RAW_RECORDER.md).

[Detailed configuration and launcher setup](reference/LOCAL_SETUP.md) · [Development status](STATUS.md)
