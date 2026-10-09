# Implementation plan

The repository begins with the first stage below. Each stage must leave a
usable, tested increment; audio acceptance is separate from software evidence.

1. **Standalone foundation (implemented):** pinned Rust project, private data
   root and configuration, exact route resolution, exclusive transport state
   and asynchronous shutdown contract, 40×13 touch/keyboard shell, CLI and
   terminal restoration. No live audio is enabled in this stage.
2. **Recording and recovery (raw host library implemented):** the independent
   [raw library](RAW_RECORDER.md) provides bounded queues, PCM24 stems, readiness,
   source-frame journaling, integrity and evidence-preserving copy recovery.
   Standalone routes/stereo identities, full take schema and shell/device wiring
   remain. Adapt only raw capture pieces from SHR, with
   provenance; bounded interleaved SPSC, startup handshake, one worker, mono
   24-bit WAVs, native versioned manifest, no-replace publication, descriptor
   based safe recovery. Test distinguishable channels, partial rename retries,
   malformed/symlink inputs, low space, startup/disk/flush failures, limits,
   overflow, xruns, and disconnects before JACK integration.
3. **Synchronized playback:** validate native and SHR format-1 takes read-only;
   bounded disk prefetch and common callback transport including seek, EOF,
   underrun, deliberate damaged-take selection, and rate mismatch rejection.
   Add exact output assignment and an explicit stereo audition mix with
   per-stem level/pan/mute and deliberate headroom.
4. **Complete gadget workflow:** exact JACK attachment with no server startup,
   persistent cancellable source/output setup, take selection, 18 circular LED
   meters, configured ALSA Sequencer input, shared action dispatch, graceful
   producer quiescence/finalization and signal handling. Finish touch/controller
   reachability and error-path tests at 40×13.
5. **Release acceptance:** build standalone release, document local setup and
   recovery commands, run normal production checks. Separately authorized Pi
   sessions measure sustained native 18-channel capture/playback, storage,
   routing, xruns, temperature and resource headroom. Long soaks stay opt-in.

## Reference decisions

Read SHR's AGENTS and workspace handoff before inspecting the named recording,
MR18, presentation, controller and September 5 review/implementation sources.
SHR's build restrictions apply to that reference checkout; no changes or builds
are performed there. This project's own instructions authorize local validation.

- `audio_recorder.rs` contains raw stems and a separate final stereo recorder;
  `recording.rs` is MIDI Idea recording. Neither the final mix nor MIDI recorder
  belongs in this app's raw take engine.
- Recovery must accept exactly one temporary or final file per stem, validate
  every stem first and retry across partial publication (review CR-03).
- Callback mutable state cannot be traversed from the UI, even if some members
  are atomic (CR-04). Publish separate snapshots.
- Only a successful writer startup handshake may enable capture (CR-10).
- SHR loop lifecycle is reference material, not a synchronized take player.
- Use one canonical action table for visible controls and dispatch. Preserve
  stable selection and remembered exact routes when devices disappear.

See [development status](STATUS.md) for delivered behavior and evidence.

## GigPies integration planning — 2026-10-04

[Owning GigPies plan](GIGPIES_IMPLEMENTATION.md) owns module-only plans and implementation progress in the same task cards.
Shared integration work is linked to its GigPies owner; dated evidence above
retains its original scope.
