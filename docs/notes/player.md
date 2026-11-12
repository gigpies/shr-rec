# Playback ownership and format compatibility

The standalone player deliberately uses the existing native raw version-1
format rather than inventing a second take format while capture UI is pending.
The immutable initial native session manifest always says incomplete; the final
result and any failure marker determine playback integrity. Copy recovery emits
recovered-incomplete, which requires deliberate selection.

SHR format-1 omitted the final RIFF padding byte for odd-length PCM24 data.
Read-only compatibility accepts exactly that final legacy data-tail exception;
new native files and interior chunks stay strict. Never fix the source on open.

Callbacks exclusively own Render inside stable UnsafeCell storage. Device owners
must use separate Shared atomics and immutable port/edge snapshots, even when a
field inside callback storage appears read-only. Close JACK before freeing any
callback or notification data, including failed activation/connection paths.
Seek discards the old stream only after quiescence and rebuilds every stem at one
frame, preserving play/pause. Playback master gain stays unity; the explicit
stereo mix uses 0.5/N per-stem headroom, independent of mute.

Behavior and test commands belong in [the playback reference](../PLAYBACK.md).
Current work and acceptance belong in [the owning plan](../PLAN.md#sp-03--task0028-standalone-playback),
not this note. Hardware acceptance cannot be inferred from fake JACK/null tests.
