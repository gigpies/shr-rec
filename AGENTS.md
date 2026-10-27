# shr-rec — standalone multichannel recorder/player

## Product and scope

Build a focused recording/playback gadget extracted from the experience and
suitable code of SHR-DAW's multitrack recorder. It is a separate application,
not a DAW feature, plugin, or SHR subprocess. The owner's intended destination
for multichannel rec/play is this app. Removing the old DAW feature is a later
separate task; do not change SHR-DAW in this project.

Record a band, rehearsal, mixer, or other multichannel source as one take;
then select and play that take with every stem on one common timeline. Keep
setup, arm, meters, record, stop, take selection, play, pause, seek, and output
assignment reachable at 40×13 without a computer keyboard. No tracker,
synthesis, arrangement editor, effects rack, destructive audio editor, or
full DAW mixer. Overdub/punch-in and simultaneous record/play are deferred
unless separately requested; initial recording and playback are exclusive.

## Recorder requirements

- Preserve sample-aligned start/stop and equal frame counts for all armed
  channels. Reuse the proven shape: one callback, a preallocated interleaved
  SPSC ring, and an off-thread writer. A period enters wholly or is rejected
  wholly; never drop frames from only one stem or hide timing gaps.
- Interface-neutral exact source assignment, stable channel IDs, readable
  labels, optional stereo pairing, per-channel arm, arm resolved/all-off, and
  refresh without overwriting remembered routes. Armed unresolved inputs block
  start. Establish an 18-channel Pi target with a bounded architecture up to
  the existing 64-channel ceiling; the ceiling is not a performance promise.
- One mono 24-bit PCM WAV per channel plus a versioned manifest, one sample
  rate, track order, and shared frame count. Preserve source/channel identity,
  actual versus preferred ports, stereo groups, and integrity counters.
- New takes never overwrite an existing take. Preserve temporary-take,
  atomic no-replace publication, incomplete/recovered status, and restartable
  recovery across partially renamed stems. Validate all stems before repair;
  reject ambiguous, malformed, newer-version, or symlink-escaping input.
- Detect storage failure, low space, RIFF size limits, ring overflow, xruns,
  disconnects, oversize callbacks, and flush/publication failure. Stop safely
  and show what was retained and where. Never label a damaged take complete.
  Make file-size/time limits visible before recording; any later segmentation
  must preserve alignment and an explicit manifest timeline.
- Reuse the compact setup view and native 18-meter overview as appropriate.
  Metering must not create a second capture owner during a take. Raw input
  monitoring stays external by default; recording must not create a hidden
  dry path or a feedback loop.

## Playback requirements — new work

SHR's documented raw recorder does not implement a multitrack take player.
Its WAV loop code is a reference for streaming/lifecycle, not proof that
synchronized playback already exists. Build and validate playback explicitly.

- Open the native take format and support read-only import/open of existing
  SHR format-1 `session.json` takes and mono stems. Never require `.shsong` or
  launch SHR. Never modify the source take merely by opening or playing it.
- Validate manifest, files, rates, lengths, and channel order before activation.
  A recovered/incomplete take remains visibly marked and requires deliberate
  selection; do not disguise damaged data as a normal complete take.
- Stream from disk through bounded prefetch buffers. All channels start,
  pause/resume, seek, and stop on one callback timeline. Long takes cannot be
  fully loaded into RAM. A seek flushes/reprimes all stems together; EOF and
  underrun have deterministic whole-timeline behavior and visible status.
- Offer exact stem-to-output assignment for multichannel playback/virtual
  soundcheck, plus an explicit stereo audition mix for a two-output card.
  Never infer that a recorded input number is the desired playback output.
  A stereo mix needs simple per-stem level/pan/mute and deliberate headroom;
  it must not silently sum many full-scale stems or normalize their files.
- Reject an unsupported take/JACK sample-rate mismatch clearly for the first
  version. No implicit resampling, time stretching, clock drift, or missing
  channel substitution. Playback and raw-input monitoring stay separate.
- Exit/launcher return while recording offers Stop and finalize / Cancel.
  Graceful shutdown must quiesce producers, drain/finalize recoverable work,
  then release owned resources. A forced crash must leave recoverable state.

## Starting references

Read these focused sources before extracting code; current source and tests
win over historical documents:

- `../shr-daw/docs/MULTITRACK_RECORDING.md`
- `../shr-daw/src/audio_recorder.rs` and `../shr-daw/src/recording.rs`
  (distinguish raw stems from the separate final stereo recorder)
- `../shr-daw/src/loop_player.rs`, `../shr-daw/src/jack.rs`
- `../shr-daw/docs/MULTITRACK_PRESENTATION.md`,
  `../shr-daw/docs/MR18_TEST_PLAN.md`
- `../shr-daw/docs/CODE_REVIEW_2026-09-05.md` and its implementation record,
  especially recorder recovery, writer startup, and callback ownership
- `../shr-daw/docs/CONTROLLER_INTERFACE.md` and controller profile/learn code

## Build order and acceptance

First define the standalone session/state and launcher-exit boundary, then
port recording and recovery with synthetic tests. Add synchronized playback
and routing next, followed by the complete keyboardless 40×13 workflow.
Validate distinguishable synthetic channels through capture, publication,
recovery, and playback: no swapped channels or frame drift. Cover partial
finalization, writer startup failure, disk failure, underrun, seek, exit during
a take, absent devices, and exact-route recovery. Run short deterministic
production checks normally; keep long disk/18-channel soaks opt-in.

The deliverable is a standalone release binary with documented local setup,
private configuration, format compatibility, recovery, and test commands.
Native 18-channel sustained recording and playback, real storage load, and
hardware routing are separate acceptance gates. Do not claim them from mocks.

## Shared platform and working agreements

- Work in this project only. Other Codex sessions will work independently in
  sibling projects. Existing repositories are read-only references: inspect
  their instructions before borrowing code, preserve licence/provenance, and
  copy/adapt only the pieces needed here. Do not modify SHR-DAW, remove its
  features, create shared libraries in sibling folders, or depend on another
  new project's implementation being ready.
- Target this Raspberry Pi 5 (2 GB RAM, active cooling, NVMe), 64-bit Linux,
  Rust, ratatui/crossterm, JACK audio, and ALSA Sequencer MIDI where needed.
  Start with the existing exact Rust 1.97.1 pin; adopt changes deliberately.
  Prefer a small native application with few dependencies.
- The touchscreen terminal is **40 columns × 13 rows**, not a desktop UI
  shrunk afterward. Preserve the existing 480×320 tty and
  `Uni2-TerminusBold24x12` font. Touch is terminal mouse input. Normal screens
  reserve row 13 for one shared useful status/fault row and rows 11–12 for
  contextual controls. A deliberate meter view may use a different body but
  must keep visible actions and the shared status row. Use circular LED meters.
- Essential operation must work without a computer keyboard: touch and a
  configured MIDI controller/keyboard, including selection, confirm, cancel,
  stop, and exit. Supply keyboard equivalents. Keep labels literal, touch
  targets usable, selections stable, and edits cancellable. Optional naming
  must never block operation; generated names are acceptable.
- Device identities, routes, controller mappings, paths, and installation
  choices belong in private configuration. No hardcoded personal equipment.
  Keep user data outside tracked files; never copy sibling private `user/`
  state, mappings, recordings, presets, logs, or downloads.
- Prepare data off the audio thread. Callbacks must not allocate/deallocate,
  lock, access files, log, format, spawn, wait, or perform unbounded work.
  Bound queues, memory, per-period work, and recovery. Publish meters through
  snapshots/atomics without aliasing mutable DSP across threads.
- Attach to the existing JACK server; never start, restart, or reconfigure it
  automatically. Own only this app's clients, processes, and exact connections.
  Missing or ambiguous required routes must be visible and must not silently
  resolve to another channel. Keep the terminal usable when audio is absent.
- Use release artifacts for live performance acceptance. Measure on the Pi;
  compilation, offline renders, and callback simulations do not establish
  whole-system latency, xruns, thermal headroom, or listening quality.
- Agent owns test selection. During implementation use focused production
  regressions; run the full normal suite when engine/rendering, shared schema,
  persistence, routing, concurrency, or safety changes. Keep unit, contract,
  recovery, bounded-audio, and 40×13 interaction tests in the normal suite.
  Historical auditions, exhaustive matrices, long soaks, and evidence
  renderers are opt-in with a documented command. Report what ran and skipped.
- Implement and verify ordinary local work without repeated confirmation.
  Creating this brief does not authorize live audio, MIDI transmission,
  recording, service/autostart changes, or borrowed-hardware tests: prepare
  those paths for a separately authorized on-device session. Do not publish
  new repositories or deploy services merely because a reference repo does.

## Shared launcher boundary

Use stable gadget IDs `rec`, `fx`, and `daw`; `go` is the launcher. New gadgets
must run directly without the launcher and return to their caller on a visible
keyboardless Exit action. Expose ordinary executable/arguments, `--help`,
`--version`, a configurable private data root, and a documented graceful
shutdown behavior. Do not require a launcher SDK, daemon, network API, or a
shared crate. The launcher owns launching/waiting; the gadget owns its audio,
state, safe stopping, and terminal restoration. Restore raw mode, cursor,
mouse reporting, and the alternate screen on normal exit and handled errors.

## GigPies task tracking

Use `docs/GIGPIES_IMPLEMENTATION.md` for GigPies work owned here. Keep each task plan,
implementation state, acceptance checklist, evidence and next action in the same
card; update it with the change. Shared integration tasks have one card in
GigPies, linked from contributor plans. STATUS, maps, handoffs and knowledge notes
route to task owners or preserve dated evidence; never mirror current task state.
Archive closed cards once; keep the active queue limited to open work. Reference
projects do not become GigPies runtime modules merely because code is reused.

## Reader-facing documentation

Keep README short and visual. Route musicians through the operating guide;
assume musical knowledge. Put protocols, implementation detail, validation and
work logs in engineering references or archives. Update the existing owning
guide instead of appending feature histories to the README.
