# Architecture

## Delivered foundation

`config` validates the bounded input list and resolves exact source preferences
without modifying them. Deserialization rejects unknown fields and versions;
reads are bounded and descriptor-checked. Explicit setup uses create-new files.

`session` owns exclusive transport states:

```text
Stopped -> Starting -> Recording -> Finalizing -> Stopped
Stopped -> Playing <-> Paused -> StoppingPlayback -> Stopped
Stopped -> Exited
```

Effects define the engine adapter's lifecycle. Recording readiness follows
worker startup, and completion follows producer quiescence and worker join.
Late readiness after stop/failure is rejected. Each capture has a monotonically
increasing operation ID carried by preparation/finalization effects and worker
readiness/completion. Retired-operation messages are rejected even after a new
capture starts. An adapter must still quiesce and join the old worker before
acknowledging completion and starting another operation.

Exit during Starting/Recording opens confirmation. Cancel preserves capture;
Stop and finalize waits in Finalizing. Exit during finalization also waits.
Only a complete retained take permits an automatic return after finalization;
an incomplete result remains Stopped with its path and detail retained.
Playback Exit waits for a separate quiescence acknowledgment.

`ui` holds stable selection and shared action tables for touch labels and hit
targets. `playback_ui` drives the take browser, cancellable exact output/stereo
mix drafts, private route saving and transport. Session follows actual playback
state; recording remains unavailable rather than presenting simulated capture.
An injectable device boundary runs the same workflow against real prefetch/null
callbacks in offline tests.

`main` handles ordinary CLI arguments, private-root/library selection and RAII
terminal restoration. Playback opens JACK only on explicit discovery/start.
Signal handlers set a flag; the event loop quiesces playback and returns through
terminal cleanup. Error unwinding also drops the playback owner.

## Independent raw recording boundary

`recorder` now implements a separate host-fed raw worker and versioned C ABI;
see [its complete lifecycle and format contract](RAW_RECORDER.md). It owns no
device. One producer supplies interleaved f64 periods through two preallocated
rtrb queues, committing samples before their source-frame descriptor. The worker
writes aligned PCM24 mono stems and a source timeline journal. Setup waits for
writer readiness; shutdown requires producer quiescence, drain and join.
An immutable incomplete manifest precedes capture; errors never count as a
successful take. Recovery validates every source stem, holds directory-relative
descriptors and copies common frames to a new destination, preserving originals.

The raw library is independent of the existing shell session controller.
It neither enables Record in the UI nor claims the future full standalone take
schema or source-route persistence. GigPies loads the cdylib through
versioned symbols; there are no sibling path dependencies.

## Remaining standalone boundaries

Capture: prepare routes, files, buffers and callback storage off-thread. One
callback exclusively owns mutable capture state and pushes whole interleaved
periods to an SPSC ring; an ordinary worker writes every mono stem. Separate
atomics/snapshots carry meters and faults. No callback allocation, deallocation,
locks, I/O, formatting, logging, spawning or waiting.

Persistence: native versioned manifest with actual/preferred identity, stereo
groups, common frames and integrity counters. Temporary takes and no-replace
publication; validate every stem before any recovery mutation. Descriptor-based
path handling must protect against symlink escapes and partial rename retries.
The raw host format/recovery above is implemented; a CLI recovery command and the
full standalone take format remain unimplemented.

Playback is implemented separately from the raw C ABI: `take` validates native
raw version 1 and SHR format-1 sources read-only; `playback` provides bounded
interleaved disk prefetch, off-thread routing/mixing and one callback cursor.
`jack_playback` dynamically attaches with no-start-server behavior and closes the
client before releasing callback storage. UI reads only shared atomics and
immutable routing snapshots. Seek quiesces and rebuilds every stem together.
See [playback](PLAYBACK.md) for precise bounds, format checks, headroom, faults,
shutdown and reproducible tests. Native 18-channel sustained operation remains
a hardware gate; the 64-channel ceiling is an architectural bound.

JACK: attach with no-start-server behavior, own only exact app connections,
and retain an operational terminal when audio is absent. ALSA Sequencer input
must feed the same action dispatcher through a bounded queue; MIDI transmission
and profile learning remain deferred.
