# Standalone take playback

[Operating guide](LOCAL_SETUP.md) · [Architecture](ARCHITECTURE.md)

The standalone `shr-rec` executable opens takes directly. It does not launch
SHR-DAW, require a song file, or depend on GigPies. The raw recorder and its C ABI
are unchanged. Standalone capture, input monitoring and MIDI remain separate work.

## Formats and validation

- Native raw version 1: `session.json` (`format: shr-rec-raw`), `result.json`,
  and `channel-001.wav` onward in numeric channel order. The immutable initial
  manifest's `incomplete` status is not the final result. Final status and
  integrity come from `result.json`; a `failure.json` marker also makes the take
  suspect. Native raw files have channel numbers rather than full source labels.
- SHR format 1: `session.json`, `tracks` in manifest order, and the exact
  `wav_file` for each track. `version` is accepted as the historic alias for
  `format_version`. Source identities and stereo roles are validated but never
  interpreted as playback destination numbers. Stereo groups need one left and
  one right stem. Legacy SHR's omitted final odd-byte WAV padding is accepted
  read-only; interior chunks and native padding remain strict.

Both require 1–64 aligned, finalized mono PCM24 RIFF/WAVE stems, one supported
8–384 kHz rate, nonzero shared frame count, matching headers/metadata, distinct
track IDs, files and inodes. Unsupported versions/encodings, inconsistent duration,
unsafe names, symlink members, nonregular files, malformed chunks and duplicate
final/partial stems are refused. No missing-stem substitution, implicit trim,
resampling, time stretching or repaired source files. A native take still owned
by capture/recovery is refused through the directory lock. Incomplete/recovered
but structurally valid takes require ACCEPT, retain a visible integrity warning,
and are revalidated before every start or seek. Partial headers/files require
explicit recovery first; opening never performs recovery.

Manifest reads are capped at 256 KiB and duplicate JSON keys are rejected at
every nesting level. Discovery scans one directory level,
refuses more than 4096 directory entries or 1024 takes, skips directory symlinks,
and keeps selection by path on refresh. `--open` bypasses discovery for one take.
All source members open relative to a held directory descriptor using
`O_NOFOLLOW`, `O_NONBLOCK`, `O_CLOEXEC`, regular-file checks and a shared directory
lock. Only explicit SAVE writes the player's separate private `playback.json`.
The library root is a trusted operator-selected path; source media should remain
unchanged while playing. Descriptor pinning prevents path replacement from
redirecting already-open reads; external in-place edits are not authenticated.

## Timeline and disk worker

`playback::prepare` validates the requested JACK rate and explicit routes off
callback, allocates one 32,768-frame interleaved SPSC ring and primes it on one
worker thread. The worker reads each stem in 1024-frame blocks and renders the
selected routing/mix off callback. Startup is paused, with at least 31,744 frames
(or the complete shorter tail) primed, and a five-second startup deadline.
The UI waits for preparation; it does not claim transport until preparation
succeeds. Long files are never loaded in full.

The ring uses at most 8 MiB (64 output channels); worker sample scratch uses at
most 256 KiB plus 3 KiB decoding scratch. Track descriptors, routing matrices,
metadata and callback storage are separately bounded by the 64-channel limit.
These bounds are architectural, not a promise of Pi throughput at 64 channels.

One callback consumes the entire requested period across every output and
publishes one common cursor. Supported periods are 1–8192 frames. Pause emits
silence and consumes nothing; resume continues the next common frame. Stop
quiesces/closes the JACK client and joins the worker, discards prefetched data,
and resets to frame zero. Seek stops/quiesces the old callback, joins its worker,
reopens/revalidates all stems, and primes fresh queues at one target frame. It
preserves playing versus paused state; stopped seeks move the next start position.
Five-second seeks clamp to the first/last playable frame.

At EOF, the final partial period plays the common tail and silences its remainder;
the cursor ends at the exact shared frame count. UI closes the client/worker and
shows End of take; PLAY starts again at zero. An underrun silences the whole
period, advances no stem/cursor, and latches a fault. No automatic restart or
late-channel catch-up. A read failure also silences/stops the whole timeline and
preserves an owner-thread diagnostic. STOP resets; deliberate PLAY can retry at
the retained frame after a fault. Files are never changed on failure.

## Outputs and mix

Exact mode assigns one unique, fully named JACK audio input destination to each
stem in take order, at unity gain. There is no input-number fallback and no
inferred adjacent port. Stereo audition assigns two distinct exact destinations
and exposes each stem's level (0–100%, 10% steps), pan (left to right, quarter
steps) and mute. Linear pan has unity gain at centre on both sides before the
explicit mix headroom. Every stem gets fixed **0.5/N gain**: 1/N for the total
stem count, including muted stems, plus a 6 dB reserve below full scale. This
prevents coherent full-scale sums and rounding from clipping. Muting never
changes another stem's gain. Master software playback volume stays 100%; files
are not normalized. The musician sets listening loudness on the amplifier.

OUTPUTS creates a cancellable draft; MODE switches exact/stereo, ASSIGN cycles
current exact destination names, PREV/NEXT chooses a stem or stereo side. MIX
edits the stereo draft. BACK from MIX returns to outputs. APPLY validates and
uses the draft for this run; CANCEL restores the previous assignment. SAVE
validates and writes private preferences for this take path and ordered IDs,
then marks that draft saved. Reopening restores only a matching take/identity
list. Loading preferences does not connect or substitute missing ports.
Settings are bounded, versioned, checked against symlinks/nonregular files,
written with mode 0600 through a create-new temporary and rename/sync. A stale
`.playback.json.part` blocks SAVE for operator inspection. Only the last saved
take's routes/mix are retained; no routes are written into a take.

## JACK and shutdown

The dynamic system `libjack.so.0` boundary follows SHR's existing attachment
approach, without adding a Rust dependency or borrowing private settings.
Discovery and playback use `JackNoStartServer | JackUseExactName`. This app owns
only its `shr-rec-playback` client, ports and exact connections. Startup happens
paused/silent; every route must connect before PLAY. Missing library/server,
busy client name, missing exact outputs and rate mismatch remain visible errors.

The callback exclusively owns its `Render` inside stable `UnsafeCell` storage;
the UI never traverses it. Separate immutable port/edge snapshots and shared
atomics carry transport and faults. Callback paths do not allocate/free, lock,
read disk, format, log, spawn or wait. A graph callback catches disconnection of
an exact owned edge; unrelated graph changes do not stop playback. Shutdown,
xrun or JACK rate changes stop the timeline. A non-real-time poll also checks
exact edges. Unsupported periods return a JACK process error to remove the
client from the process graph without traversing an unbounded buffer.

Every startup error closes JACK before releasing callback/notification storage.
Normal STOP, EXIT, handled errors, SIGINT, SIGTERM and SIGHUP quiesce playback,
close only this client's resources and join the reader. Cancellation is checked
between bounded disk blocks; shutdown waits for any in-flight regular-file I/O. Terminal raw mode,
mouse reporting, cursor and alternate screen are restored. Forced termination
cannot run cleanup but the source take is read-only.

## Reproducible offline validation

```sh
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo test --locked --test playback --test playback_ui --test playback_callback --lib -- --test-threads=1
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo test --locked --all-targets -- --test-threads=1
cargo fmt --all -- --check
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo check --locked --all-targets
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo clippy --locked --all-targets -- -D warnings
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo build --locked --release
```

Use the shared build lock for the entire build sequence on the lab Pis. Tests
create/remove their synthetic fixtures in temporary storage. They exercise native
capture → publication/recovery → playback, SHR compatibility, bounded streaming,
18/64-channel identity, pause/seek/EOF, whole-period underrun, startup/read/device
failures, no-allocation callbacks, exact connections, teardown and the actual
40×13 touch workflow with a null device. The adapter's function table is replaced
by a fake JACK implementation in unit tests; no real JACK server is contacted.
CLI/PTY tests cover direct launch and terminal restoration. They do **not** prove
physical routing, listening quality, latency, xruns, storage-load endurance or
native 18-channel sustained performance. Those remain separately authorized
hardware gates; no hardware reservation is implied.
