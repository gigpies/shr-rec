# Development status

Updated 2026-10-04. Stage 1 and the independent raw-library part of stage 2 in
[the plan](PLAN.md) are implemented.

## Task0009 REC-01 / REC-02 software continuation

Operation-correlated Session effects/readiness/completion now fence retired
workers. Stop during preparation waits for adapter cleanup; duplicate Stop and
completion cannot issue a second finalize. The additive v2 C observer owns
independent refcounted atomics and can outlive the consumed v1 recorder. It
reports accepted versus all-stems-and-journal written frames, unknown durability,
integrity counters, first host fault, writer fault and finalized outcome. The
v1 ABI and raw format remain unchanged. Header output-size rejection and
observer lifetime rules are explicit in [RAW_RECORDER](RAW_RECORDER.md).

The E06 JSON corpus exercises real session behavior, and the direct C ABI runner
verifies 96 distinguishable exact PCM24 frames per stem. Tests cover concurrent
observation, retained lifetime, no-write bounds rejection, complete/incomplete/
error/drop/panic outcomes and allocation-free callback paths. **30 normal tests pass**, along with warning-denied all-target Clippy and the
release build on exact Rust 1.97.1. Final commands/results are recorded in the
owning GigPies plan. These synthetic checks
do not replace the retained hardware evidence below. Shell recording/playback
remain unavailable until a real standalone adapter is implemented.

## Independent raw recorder

The [raw recorder library](RAW_RECORDER.md) now writes aligned mono PCM24 WAVs
from host-provided f64 interleaved periods. It supplies versioned C ABI symbols
through `libshr_rec.so`, bounded preallocated rtrb queues, writer readiness,
source epoch/frame journaling, create-new storage, fault counters, draining
finalization and recovery that copies common frames while preserving originals.
Source valid bits are declared separately from PCM24 container width. It creates
no audio device/client, route, service or monitoring path. The shell's Record
and Play controls remain unavailable.

At the pre-task0009 raw-library checkpoint, exact rustc 1.97.1 passed formatting,
all-target tests, warning-denied Clippy and
the release build with `CARGO_INCREMENTAL=0`. **23 normal tests passed**: the prior
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

#### H8: exact recording; physical continuity remains qualified

GigPies runtime v13 (`1539915`) completed H8 at 48 kHz with 48-frame periods,
192-frame device capacity, zero silent prefill and 288-frame (6 ms) wet admission.
The owned audio thread used FIFO 20 on CPU 3, process memory locking and the
trial's temporary `compact_unevictable_allowed=0` setting. The recorder remains
source `c390305`, release SHA-256
`6d3b0cfcb8134ffb6d0851fc9a3259b02cd57c36ac1ac4752e6109ff5b3e718a`.
Separate offline 48/96-frame ABI checks also passed without changing that library.

The 600-second take finalized complete with **28,800,000 frames per stem**.
All eight PCM24 hashes, native ADC payload hashes, dry/prepared-DAC replay and
journal were exact. There were zero recording drops, gaps, errors, USB xruns,
queue errors or missing/expired wet returns; all 600,000 returns arrived.
Render time peaked at 289.665 microseconds, with zero recorded render faults,
context switches or measurement errors.

Physical acceptance remains qualified. Of 11,977 windows, 11,975 met the
correlation threshold, spanning 249–251 frames (5.1875–5.229167 ms); two 100 ms
windows were weak. Refinement with 10 ms windows found one-frame offset increases
near 513.71 seconds (correlation 0.580) and 520.30 seconds (0.782). No persistent
step reached 24 frames, but the cause, exact converter sample continuity and
clock lock remain unresolved. `requires_review` stays true. Exact stored samples
do not prove physical sample continuity. The initial one-second quiet interval
and final 100 ms are outside the reference-window coverage.

Two 16-second packet-fault/restart trials each retained 768,000 exact frames,
with continuous raw/dry recording, zero xruns and verified wet recovery. A forced
xrun finalized 96,000 exact retained frames as incomplete with host fault 2;
a new 30-second session retained 1,440,000 exact frames. The host restored its
memory locks and scheduling, restored the compaction setting to 1, and released
task resources after the trials.

Earlier failures remain evidence. H7's 600-second take had every physical window
at 249 frames but failed 4 ms wet admission with two missing/two expired returns,
4.156 ms maximum RTT and 189 differences from the no-fault prepared-DAC reference.
Its stored hashes were exact. H5/H6 retained incomplete takes; memory locking
alone did not prevent H6's 6.219611 ms `migration_entry_wait_on_locked` at the PA
library's `memset` GOT entry. No recorder-worker cause was established. The
192-frame device capacity was selected over 144 after the smaller capacity's
30-second trial had three weak startup windows and a higher settled offset.

Runtime v12 onward labels legacy DAC stems as **intended/prepared output**.
Their exact recording does not establish output-write completion or analog
output continuity. The host retains a separate ADC guard block on a shutdown
alarm. GigPies `docs/AUDIO_HARDWARE.md` and its private H8 trial, fault and offset
reports own the full conditions, evidence and limits. These trials do not enable
standalone UI recording/playback or establish both-channel physical acceptance,
acoustic latency or full-show reliability.

#### Earlier v7 recording and electrical evidence

GigPies' v7 host passed a 600-second actual-device/NVMe recording at
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
physical capture-to-speaker or acoustic latency. Subsequent generated-only
electrical loopback verified output 1 to input 1. A 12-second unmodified v7 run
with real PA, FX and recording retained 576,000 frames per stem; all eight PCM24
and direct ADC hashes, dry/DAC replay and the journal were exact, with no xruns
or wet-return errors. The left 997 Hz return measured -0.674 dB gain. The right
1499 Hz return was 68.77 dB lower and remains unresolved; the user suspects its
cable and selected the working channel. A separate PN probe's 2739-frame /
57.0625 ms lag includes startup and 2688 frames of playback prefill, so it is
not an isolated converter or capture-to-speaker latency measurement. No ADC
monitoring/feedback path was enabled.

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
