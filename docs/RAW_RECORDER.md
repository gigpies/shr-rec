# Raw recording library

The independent raw recorder is implemented in `src/recorder.rs`. Build the
Rust library and Linux C ABI with `CARGO_INCREMENTAL=0 cargo build --locked
--release`. The shared library is `target/release/libshr_rec.so`; the versioned
declarations are in [include/shr_rec.h](../include/shr_rec.h). This library opens
no audio device. The host supplies normalized f64 samples and owns device access,
the source-frame clock, exact source labels and producer shutdown. The terminal
shell remains disconnected from recording/playback.

## Lifecycle and callback contract

1. Create with a **new** take-directory path, sample rate, ordered channel count,
   maximum callback frames, queue-block count and source epoch. Creation waits
   for the worker to prepare files, synchronize its incomplete manifest and
   allocate scratch storage. Existing paths are never overwritten.
2. Optionally declare nominal selected source payload bits using `set_source_bits`.
   This metadata does not measure effective converter resolution (ENOB). A 16-bit
   source remains 16-bit in the result even though WAV storage uses PCM24. Omitted
   precision remains unknown. The host retains physical route/channel labels;
   ordered generic stem indices do not claim physical channel identities.
3. A single producer pushes frame-major interleaved doubles. Each period enters
   wholly or is refused wholly. Push and fault reporting allocate/free nothing,
   take no locks and perform no I/O. Preallocated rtrb sample and descriptor rings
   are touched during setup. Descriptor publication follows sample publication.
4. Source frames must increase without overlap within one epoch. Forward gaps
   are recorded and mark the result incomplete. The timeline journal maps every
   retained period to its absolute source frame and file-frame offset. Dropped
   periods are never silently represented as contiguous source time. Invalid,
   nonfinite, oversized or reversed periods are refused and counted.
5. Report host xruns/disconnects using `mark_fault`; the first nonzero code is
   retained. Finish the old handle when replacing the source epoch, then create
   a new one. The recorder cannot detect device faults hidden by the host.
6. Quiesce the producer, then finish off the callback thread. Finish consumes the
   handle, drains accepted work, joins the worker, writes WAV sizes, synchronizes
   files and publishes each stem and result without replacement. Return 0 means
   complete, 1 means finalized incomplete, and -1 means an error. Retain the path
   and inspect evidence on either failure. Dropping a Rust handle drains with an
   explicit incomplete fault. Never race any handle use with destruction.

Limits: 8–384 kHz, 1–64 channels, 1–8192 frames per period, 2–4096 queue blocks,
and at most 8,388,608 queued doubles (64 MiB plus descriptors and worker buffers).
These bounds do not promise sustained 64-channel hardware performance. Individual
mono RIFF files stop before `(UINT32_MAX - 37) / 3` frames, about 8.28 hours at
48 kHz. There is no automatic segmentation. Storage is checked at startup and
every 256 written blocks against a 16 MiB reserve; concurrent writers can still
consume space, so every write, finalization and synchronization error also fails
the take. PCM payload rate is `sample_rate * channels * 3` bytes per second.

## Native raw format version 1

This focused host format is separate from SHR-DAW's `session.json` format and
from the fuller planned standalone take schema (routes, stereo groups, labels).

| File | Meaning |
| --- | --- |
| `session.json` | Immutable `shr-rec-raw`, version 1, initial incomplete status, PCM_S24LE, configuration and source epoch |
| `channel-001.wav.part` | Active/interrupted first mono stem; other channels use ordered three-digit indices |
| `channel-001.wav` | Finalized mono PCM24 stem; odd byte payloads have a RIFF pad byte |
| `timeline.jsonl` | One JSON object per wholly written period: source_frame, frames, file_frame |
| `result.json` | Final complete/incomplete report, frames, epoch, optional valid source bits, dropped/gap/invalid/clipping counters and first host fault |
| `failure.json` | Best-effort evidence after write/finalization error; overrides any apparent complete result |

Storage conversion rounds `sample * 2^23` and clamps to signed 24-bit range.
Normalized signed PCM24 codes round-trip exactly, including both extrema and
the least significant bit. This is output quantization, not a claim of 24-bit
converter resolution. Clipping is counted and prevents a complete result.
An empty take is incomplete. All stems share one retained frame count on clean
finalization; a disk failure can leave unequal partial files for recovery.

A reader accepts complete only after successful finish and a valid complete
result with all stems matching. Missing/partial result, `failure.json`, failed
finish or any integrity fault means incomplete. A final directory-sync failure
can leave a visible result whose durable publication is uncertain; the finish
error takes precedence. No power-loss durability certification is claimed.

## Evidence-preserving recovery

`recover(source, new_destination)` / `shr_rec_v1_recover` takes an exclusive
nonblocking lock on the source directory. An active writer or recovery refuses
the request. It uses directory-relative descriptors with no-follow opens,
rejects nonregular files, future/unknown schemas, wrong WAV format/rate, and
requires exactly one temporary or final file per expected channel. Every stem
is validated before creating the destination.

Recovery copies the shortest common whole-frame payload, with bounded streaming
memory, into fresh files and finalizes them as **recovered-incomplete**. It never
truncates, repairs, renames or overwrites source evidence, including incomplete
sample bytes and partial publication. After interrupted recovery, retain that
destination and retry to another new path. Physical/source timeline precision
and counters cannot be inferred from crash payloads; original manifests and
journals remain the evidence. The recovered copy's empty timeline intentionally
makes no source-continuity claim. User-space crash recovery is covered by tests;
real filesystem power-loss behavior remains an independent gate.

## Validation

```sh
CARGO_INCREMENTAL=0 cargo test --locked --all-targets
cargo fmt --check
CARGO_INCREMENTAL=0 cargo clippy --locked --all-targets -- -D warnings
CARGO_INCREMENTAL=0 cargo build --locked --release
```

Normal tests cover exact PCM24 codes and channel order, odd RIFF padding, source
gaps/reversal/nonfinite/oversize refusal, whole-period overflow, startup collisions,
empty/drop/clipping faults, `/dev/full` write failure, publication collisions,
all partial-rename recovery boundaries and repeat recovery, live-owner locks,
symlink/ambiguous/future-schema/wrong-rate refusal, size limits and callback
allocation/deallocation counts. Existing CLI, terminal and transport contracts
remain in the normal suite. A process-crash check kills an owned child after a
written period, recovers all 192 common frames and verifies the original payload
is unchanged. No long soak, live device, network, playback or
acoustic test is implicit in these commands. GigPies owns separate integration
and actual-device evidence; no standalone shell audio acceptance is claimed.

## Embedded hardware checkpoint

On October 3, GigPies exercised raw-library source commit `c390305` through its
actual-device host. The accepted final v7 configuration uses AudioBox 48 kHz
stereo S32 capture, 384-frame periods, a 3072-frame device buffer and a 768-frame
(16 ms) wet-return budget, with native f64 PA `390a45f` and FX `6510ead9`.
The eight recorded stems contained two physical
ADC channels plus three stereo software taps: deterministic source stimulus,
PA dry output and samples submitted to the DAC. Submitted DAC samples establish
software output integrity; they are not a recording of the analog output.

| Experiment | Recorder result |
| --- | --- |
| Final v7 600-second NVMe take | 28,800,000 frames per stem; all eight PCM24 hashes and direct ADC-byte hashes exact; dry/DAC replay and full journal exact; no clipping, recording faults or xruns |
| Final v7 packet-fault and restart trials, 15 seconds each | Exact raw/dry continuity, zero xruns and both wet channels recovered |
| Earlier 600-second take, 192-frame period / 768-frame buffer | 28,800,000 frames per stem; all eight PCM24 payload hashes matched the host; full source journal contiguous; exact PA dry replay; zero xruns, gaps, drops, clips or finalization errors |
| Earlier two 15-second transport fault/restart trials | 720,000 frames each; exact dry and recorded samples; zero xruns |
| Forced 100 ms local driver stall | 96,000 retained frames; incomplete take with host fault 2 |
| New session after the driver stall | 30 seconds / 1,440,000 exact frames |
| Host v5 explicit MSB24 capture mapping | Separate 30-second trial passed direct ADC-byte-to-PCM24 comparison |

The final v7 soak received all 600,000 FX returns with zero missing/expired
returns or queue errors. Its host SHA-256 is
`c4b8627790797cda04dc95ff5f3c4454c46ba142f77772f8557123b50694d7c5`.
GigPies retains the recording and verification under ignored
`artifacts/audio-hardware/2026-10-03/soak-600s-buffer8-v7/`.

The first soak had one wet-network admission deadline miss at the original 8 ms
budget. That remains a retained integration failure despite intact recording.
The final v7 fault checks and soak establish the observed revised 16 ms
wet-return configuration with its larger device buffer; they do not convert
that admission budget into a physical end-to-end latency measurement.

The USB descriptor reported 24 bits while ALSA reported 32 significant bits;
observed low bytes included 0 and 3. Host v5 therefore explicitly selects the
most significant 24 bits from the S32 payload before supplying ADC samples to
this library. Its direct byte comparison validates that mapping separately from
the earlier host-to-WAV hash checks. `source_valid_bits` describes nominal
selected payload precision; it is not a measurement of ADC effective resolution.
Mixed physical/software channel identity stays in the host's source-to-stem map.

GigPies `docs/AUDIO_HARDWARE.md` owns the complete measurements, failures and
private evidence locations. This checkpoint establishes the embedded library's
observed recording behavior in that two-input configuration. Both physical
output-to-input connections are now present; a separate generated-signal test
is being prepared without an ADC feedback path, and physical acceptance remains
unmeasured at this checkpoint. Standalone UI recording/playback, native eight-
or eighteen-input capture, analog loopback, acoustic latency and full-show
reliability remain separate acceptance gates.
