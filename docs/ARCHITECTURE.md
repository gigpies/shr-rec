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

Effects tell a future engine adapter what to do. Recording readiness follows
worker startup, and completion follows producer quiescence and worker join.
Late readiness after stop/failure is rejected. An adapter must join an old
operation before starting another; this initial protocol has no cross-operation
message IDs and must not receive delayed messages from retired workers.

Exit during Starting/Recording opens confirmation. Cancel preserves capture;
Stop and finalize waits in Finalizing. Exit during finalization also waits.
Only a complete retained take permits an automatic return after finalization;
an incomplete result remains Stopped with its path and detail retained.
Playback Exit waits for a separate quiescence acknowledgment.

`ui` holds stable selection and one shared action table for touch labels and
hit targets. The current executable creates no engine, so Record/Play remain
unavailable and no synthetic recording state is presented as real capture.
The session protocol's active states are exercised in tests only. Wiring its
effects and live status to engine ownership is required before enabling audio.

`main` handles ordinary CLI arguments, private-root selection and RAII terminal
restoration. It owns no worker, audio client or MIDI port in this milestone.
Signal handlers set a flag; the event loop returns through terminal cleanup.

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
schema, source-route persistence or playback. GigPies loads the cdylib through
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

Playback: bounded interleaved prefetch from validated mono stems; one common
callback cursor with coordinated pause, seek/reprime, EOF and underrun. No
full-take loading, implicit resampling or input-number-to-output inference.
SHR format-1 support must be read-only and tested explicitly. The native take
schema remains to be defined; the shell does not yet open SHR takes.

JACK: attach with no-start-server behavior, own only exact app connections,
and retain an operational terminal when audio is absent. ALSA Sequencer input
must feed the same action dispatcher through a bounded queue; MIDI transmission
and profile learning are outside this first foundation.
