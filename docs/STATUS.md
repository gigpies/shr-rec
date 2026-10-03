# Development status

Updated 2026-10-03. Stage 1 and the independent raw-library part of stage 2 in
[the plan](PLAN.md) are implemented.

## Independent raw recorder

The [raw recorder library](RAW_RECORDER.md) now writes aligned mono PCM24 WAVs
from host-provided f64 interleaved periods. It supplies versioned C ABI symbols
through `libshr_rec.so`, bounded preallocated rtrb queues, writer readiness,
source epoch/frame journaling, create-new storage, fault counters, draining
finalization and recovery that copies common frames while preserving originals.
Source valid bits are declared separately from PCM24 container width. It creates
no audio device/client, route, service or monitoring path. The shell's Record
and Play controls remain unavailable.

Exact rustc 1.97.1 passes formatting, all-target tests, warning-denied Clippy and
the release build with `CARGO_INCREMENTAL=0`. **23 normal tests pass**: the prior
13 foundation checks plus ten raw engine/recovery/callback checks. New coverage
includes exact PCM24 extremes and channel order, odd RIFF padding, forward gaps,
backwards/nonfinite/invalid input, whole-period overflow, startup/empty/drop/
clipping faults, `/dev/full` write failure, no-replace publication collision,
partial-publication recovery, repeated recovery, malformed/symlink/live-writer
refusal and callback allocation/deallocation checks. A separate child process
is killed after writing a real period; copy recovery retains its 192 common
frames and leaves the original payload exact. This is process-crash evidence,
not filesystem power-loss acceptance.

Hardware, network, long soaks, playback, acoustic and 18-channel performance
checks were intentionally excluded from this module's software pass. No
historical/exhaustive tests exist here. GigPies owns separate real-device host
acceptance and source-to-stem mapping. No physical bit depth or device continuity
is inferred from these synthetic fixtures.

### Embedded actual-device checkpoint (October 3)

GigPies' final v7 host passed a 600-second actual-device/NVMe recording at
48 kHz with AudioBox stereo S32 capture, a 384-frame period, 3072-frame device
buffer and 768-frame (16 ms) wet-return budget. Raw-library source `c390305`
ran alongside native f64 PA `390a45f` and FX `6510ead9`. Eight stems each retained
28,800,000 frames: **two physical ADC channels and six software taps** (source
stimulus, PA dry output and submitted DAC samples, each stereo). All eight PCM24
payload hashes and direct ADC-byte hashes matched; dry/DAC replay and the full
journal were exact. All 600,000 FX returns arrived with zero missing/expired
returns, xruns or queue errors. Recording had no gaps, drops, clips or faults.

The same v7 configuration passed separate 15-second packet-fault and worker-
restart trials: raw/dry continuity remained exact, no xruns occurred and both
wet channels recovered. A forced 100 ms driver stall retained 96,000 frames
and correctly marked the take incomplete. These are bounded acceptance results;
standalone UI recording and playback remain pending.

Earlier 192-frame-period / 768-frame-buffer trials are retained separately.
Their initial 600-second recording had exact PCM24 hashes, journal and PA dry
replay without recording errors, but the overall integration missed its 8 ms
wet-return target once. That failed target remains part of the evidence.

Two 15-second transport fault/restart trials retained 720,000 exact dry/recorded
frames each without xruns. A forced 100 ms local driver stall closed a 96,000-frame
take as incomplete with host fault 2; a fresh 30-second session retained 1,440,000
exact frames. Host v5 then explicitly selected the most significant 24 bits from
S32 and passed a separate 30-second direct ADC-byte comparison. Source bit
metadata describes this nominal selected payload, not effective converter
resolution. The v7 soak above completes the repeated fault and 600-second checks
for the revised 16 ms wet-return budget and larger device buffer.

The [raw-library hardware checkpoint](RAW_RECORDER.md#embedded-hardware-checkpoint)
records the interpretation limits. GigPies `docs/AUDIO_HARDWARE.md` owns the
complete metrics and private evidence paths. These results do not enable the
standalone UI or playback, establish native eight-channel capture, or establish
physical end-to-end/acoustic latency. Both physical output-to-input connections
are now present, but that separate physical acceptance has not yet been measured
at this documentation checkpoint.

## Foundation delivery (September 7)

## Delivered

- Standalone `shr-rec` executable, exact Rust 1.97.1 pin and locked dependencies.
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

Release artifact: `target/release/shr-rec` (not tracked). Run the PTY check against
that artifact with `python3 tests/terminal.py target/release/shr-rec`; this also
passed, as did the release `--version` smoke check.

No historical/exhaustive tests or long soaks exist yet. Live JACK, MIDI,
recording/playback, storage-load, physical touch/controller and borrowed-device
checks were intentionally not run. Synthetic session events do not establish
capture, file recovery, synchronization, latency or 18-channel performance.

## Remaining standalone implementation

Complete stage 2's full native take schema and stereo/source identities, then
connect the tested worker to the shell and exact device routes. The raw host
format is deliberately separate from that future standalone format. Playback,
source/output editing, persistence of UI edits, meters and ALSA controller input
remain unimplemented. No SHR take compatibility is claimed yet. Read
[architecture](ARCHITECTURE.md) before integrating worker completion or
signal-driven finalization.

The package, executable, help/version output and UI title now use `shr-rec` to
avoid the existing SoX `rec` command. Launcher ID `rec` is unchanged. All 13
normal tests, Clippy and the release build passed again after the rename.
