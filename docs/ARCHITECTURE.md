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

## Required next boundaries

Capture: prepare routes, files, buffers and callback storage off-thread. One
callback exclusively owns mutable capture state and pushes whole interleaved
periods to an SPSC ring; an ordinary worker writes every mono stem. Separate
atomics/snapshots carry meters and faults. No callback allocation, deallocation,
locks, I/O, formatting, logging, spawning or waiting.

Persistence: native versioned manifest with actual/preferred identity, stereo
groups, common frames and integrity counters. Temporary takes and no-replace
publication; validate every stem before any recovery mutation. Descriptor-based
path handling must protect against symlink escapes and partial rename retries.
No recovery command or take format is implemented in this foundation.

Playback: bounded interleaved prefetch from validated mono stems; one common
callback cursor with coordinated pause, seek/reprime, EOF and underrun. No
full-take loading, implicit resampling or input-number-to-output inference.
SHR format-1 support must be read-only and tested explicitly. The native take
schema remains to be defined; the shell does not yet open SHR takes.

JACK: attach with no-start-server behavior, own only exact app connections,
and retain an operational terminal when audio is absent. ALSA Sequencer input
must feed the same action dispatcher through a bounded queue; MIDI transmission
and profile learning are outside this first foundation.
