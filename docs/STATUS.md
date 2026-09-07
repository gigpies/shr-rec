# Development status

Updated 2026-09-07. Stage 1 of [the plan](PLAN.md) is implemented.

## Delivered

- Standalone `rec` executable, exact Rust 1.97.1 pin and locked dependencies.
- `--help`, `--version`, configurable private root and explicit create-only
  config initialization; ordinary launch does not write configuration.
- Default 18 disarmed/unassigned input slots, validated stable IDs and bounded
  config, pure exact-route resolution including missing/ambiguous refusal.
- Exclusive recording/playback state protocol with writer readiness, stop
  acknowledgment, guarded Exit, cancellable confirmation and retained-fault
  visibility. Engine operations are contracts only at this stage.
- 40×13 shell with touch/keyboard selection, session-only arm changes, shared
  status row, visible Exit and smaller-terminal fallback.
- Normal and signal-driven shell exit with terminal restoration.

## Validation

On AArch64 Linux with rustc 1.97.1 (`8bab26f4f`, LLVM 22.1.6):

```sh
cargo fmt --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
```

Normal suite: **13 tests passed**. Includes transport exclusivity, exit during
capture/startup, failed startup/retry, finalization faults, exact-route recovery,
config limits/identity/symlink/FIFO refusal, create-only permissions, CLI use
without HOME/TTY, 40×13 rendering and touch interaction. The PTY regression
launches isolated shell processes and checks keyboard Exit, touch Exit and
SIGTERM/SIGINT/SIGHUP restoration of terminal flags, cursor, alternate screen
and mouse reporting. No personal configuration is read by those fixtures.

Release artifact: `target/release/rec` (not tracked). Run the PTY check against
that artifact with `python3 tests/terminal.py target/release/rec`; this also
passed, as did the release `--version` smoke check.

No historical/exhaustive tests or long soaks exist yet. Live JACK, MIDI,
recording/playback, storage-load, physical touch/controller and borrowed-device
checks were intentionally not run. Synthetic session events do not establish
capture, file recovery, synchronization, latency or 18-channel performance.

## Next implementation

Stage 2: native take schema, bounded raw capture/writer and restartable recovery
with distinguishable synthetic-channel fixtures. Add stereo identity and
integrity counters with the take format, then connect the tested engine to the
shell. Playback, source/output editing, persistence of UI edits, meters and
ALSA controller input remain unimplemented. No SHR take compatibility is
claimed yet. Read [architecture](ARCHITECTURE.md) before integrating worker
completion or signal-driven finalization.
