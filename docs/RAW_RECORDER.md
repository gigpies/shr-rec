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

### H8 recording complete; physical continuity needs review

Recorder engine source `c390305` and release library
`6d3b0cfcb8134ffb6d0851fc9a3259b02cd57c36ac1ac4752e6109ff5b3e718a` remain
unchanged. The actual host and private offline ABI checks exercise 48-frame
blocks; the offline check also covers 96-frame blocks, all eight exact PCM24
payloads and the complete source journal. These checks establish the supplied
sample contract, not callback wall-time bounds.

GigPies runtime v13 (`1539915`) completed H8 with AudioBox 48 kHz stereo S32
capture, 48-frame periods, 192-frame device capacity, zero silent prefill and
288-frame (6 ms) wet admission. Its owned audio thread used FIFO priority 20 on
CPU 3 with process memory locking. The temporary trial guard set
`compact_unevictable_allowed=0`; the original value 1, memory-lock state and
scheduling were restored after each run. Task resources were released afterward.

| H8 check | Recorded result |
| --- | --- |
| 600-second soak | Complete, 28,800,000 frames per stem; all eight PCM24 hashes, native ADC payload hashes, dry/prepared-DAC replay and full journal exact |
| Soak integrity/network | Zero recording drops, gaps, errors, USB xruns, queue errors or missing/expired wet returns; all 600,000 return packets received |
| Render measurements | Maximum 289.665 microseconds; zero recorded render faults, context switches or measurement errors |
| Packet faults, 16 seconds | 768,000 exact retained frames; raw/dry continuity, zero xruns and verified wet recovery |
| Brain restart, 16 seconds | 768,000 exact retained frames; raw/dry continuity, zero xruns and verified wet recovery |
| Forced xrun | 96,000 exact retained frames finalized incomplete with host fault 2 |
| New session after fault | 30 seconds / 1,440,000 exact frames |

The eight stems still represent **two physical ADC channels and six software
taps**. Only physical channel 1 was exercised as the working electrical return;
right software source/output remained zero. The final soak's physical analyzer
reported 11,975 trusted windows out of 11,977, with offsets of 249–251 frames
(5.1875–5.229167 ms). Two 100 ms windows were weak. Refinement with 10 ms windows
found a one-frame increase near 513.71 seconds (correlation 0.580) and another
near 520.30 seconds (0.782). These are approximate window locations, not exact
converter discontinuity sample indices.

No persistent change reached the 24-frame threshold. Nevertheless, the cause,
exact converter sample continuity and clock lock remain unresolved, so the
physical analyzer's `requires_review` remains true. Exact stored PCM does not
prove physical sample continuity. Reference coverage excludes the first second
of quiet and the final 100 ms. The measured electrical offset includes the host,
USB and converter path; it is not acoustic or capture-to-speaker latency.

#### Retained comparisons and failures

H7's 600-second take had 28,800,000 exact retained frames per stem and all 11,977
physical windows at 249 frames. It still failed 4 ms wet admission: two missing
and two expired returns, despite all 600,000 packets arriving, with RTT reaching
4.156 ms. The prepared DAC differed in 189 samples from the no-fault replay;
the recorder's stored hashes remained exact. H8 changed only wet admission to
6 ms, preserving the dry period, device capacity and zero silent prefill.

The 144-frame-capacity comparison passed 30 seconds but had three weak startup
windows and a settled 257-frame offset. The selected 192-frame capacity showed
249 frames in every one of its 577 windows. Earlier H5/H6 failures remain
retained as incomplete takes. H6 still failed with 145,008 KiB locked: the host's
perf trace identified a 6.219611 ms `migration_entry_wait_on_locked` at the PA
library's `memset` GOT entry. This did not establish recorder-worker load as the
cause; the subsequent temporary compaction condition is part of the observed
host configuration.

The recorder's ordinary push path checks and copies f64 samples; PCM24 encoding,
stem writes and journal writes run on its independent worker. Setup touches the
complete sample and descriptor rings before capture. This avoids untouched ring
storage on wrap but does not lock memory or prove immunity to page faults,
page migration or scheduling delays. The release binary contains no ordinary
push-path allocation, mutex, wait or explicit system call.

Runtime v12 onward explicitly treats legacy DAC stems as intended/prepared
output. Exact PCM24 hashes validate recorded buffers; output-write completion
and physical ADC correspondence require separate host evidence. A shutdown
alarm can retain a separate exact ADC guard block outside the eight standard
stems. GigPies owns that guard's format and acceptance record.

GigPies `docs/AUDIO_HARDWARE.md` owns full conditions and private evidence,
including `h8-trial-summary.json`, `h8-fault-assessment.json` and
`h8-small-offset-review.json` under its ignored hardware artifacts. Standalone
UI recording/playback, both-channel physical acceptance, native eight- or
18-input capture, acoustic latency and full-show reliability remain open gates.

### Earlier v7 recording and electrical evidence

On October 3, GigPies exercised raw-library source commit `c390305` through its
actual-device host. The earlier v7 configuration used AudioBox 48 kHz
stereo S32 capture, 384-frame periods, a 3072-frame device buffer and a 768-frame
(16 ms) wet-return budget, with native f64 PA `390a45f` and FX `6510ead9`.
The eight recorded stems contained two physical
ADC channels plus three stereo software taps: deterministic source stimulus,
PA dry output and prepared DAC samples. The historical host called the final
pair submitted DAC samples; these files establish the prepared software buffer
contents, with output delivery assessed separately.

| Experiment | Recorder result |
| --- | --- |
| Final v7 600-second NVMe take | 28,800,000 frames per stem; all eight PCM24 hashes and direct ADC-byte hashes exact; dry/DAC replay and full journal exact; no clipping, recording faults or xruns |
| Final v7 packet-fault and restart trials, 15 seconds each | Exact raw/dry continuity, zero xruns and both wet channels recovered |
| Unmodified v7 generated-only electrical trial, 12 seconds | 576,000 frames per stem; all eight PCM24 and direct ADC hashes, dry/DAC replay and journal exact; no xruns or wet-return errors; channel 1 physical return verified |
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
observed recording behavior in that two-input configuration. Later generated-only
electrical probes verified output 1 to input 1 with no ADC monitoring/feedback
path. Two eight-second PN probes at -72 and -54 dBFS used the accepted 384/3072
period/buffer settings without errors. The working left path had correlation
0.799 at a 2739-frame (57.0625 ms) lag. That interval includes 2688 frames of
playback prefill, the startup call, USB transport and converters; it does not
isolate converter delay or establish capture-to-speaker latency.

The unmodified v7 host then ran for 12 seconds with real PA, FX and recording.
The left 997 Hz return measured -0.674 dB gain; the right 1499 Hz return measured
-69.440 dB, 68.77 dB below the left result. The right physical path remains
unusable and unresolved. The user suspects its cable and directed subsequent
work to the verified channel. The complete 576,000-frame software/recording
checks passed as listed above; channel 2's electrical result is not accepted.
Standalone UI recording/playback, native eight- or eighteen-input capture,
both-channel physical acceptance, acoustic latency and full-show reliability
remain separate gates.
