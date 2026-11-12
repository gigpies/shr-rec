# Implementation plan

The repository begins with the first stage below. Each stage must leave a
usable, tested increment; audio acceptance is separate from software evidence.

1. **Standalone foundation (implemented):** pinned Rust project, private data
   root and configuration, exact route resolution, exclusive transport state
   and asynchronous shutdown contract, 40×13 touch/keyboard shell, CLI and
   terminal restoration. No live audio is enabled in this stage.
2. **Recording and recovery (standalone software implemented; hardware gate open):** the independent
   [raw library](RAW_RECORDER.md) provides bounded queues, PCM24 stems, readiness,
   source-frame journaling, integrity and evidence-preserving copy recovery.
   [SP-04](#sp-04--task0029-standalone-capture) owns standalone exact capture,
   private setup, optional identity metadata, shell/device lifecycle and recovery.
   Stereo capture grouping and stopped-input meters remain. Adapt only raw capture pieces from SHR, with
   provenance; bounded interleaved SPSC, startup handshake, one worker, mono
   24-bit WAVs, native versioned manifest, no-replace publication, descriptor
   based safe recovery. Test distinguishable channels, partial rename retries,
   malformed/symlink inputs, low space, startup/disk/flush failures, limits,
   overflow, xruns, and disconnects before JACK integration.
3. **Synchronized playback (implemented; hardware gate open):** native raw
   version 1 and SHR format-1 validation/read-only selection, bounded prefetch,
   common play/pause/stop/seek, EOF/underrun/read/device faults, exact outputs and
   stereo audition with per-stem level/pan/mute and explicit headroom. The
   standalone touch workflow is wired to a dynamic no-start JACK adapter.
   [SP-03](#sp-03--task0028-standalone-playback) owns acceptance and next action;
   [playback reference](PLAYBACK.md) owns behavior and reproducible commands.
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

## SP-03 — Task0028 standalone playback

Owner: Pi4 bounded worker in canonical shr-rec; Pi5 reviews/integrates the
result. Shared audio-setup/provider work stays in the Task0028 GigPies card.
No sibling edits, recorder-engine/ABI changes, device execution or hardware
reservation belong to this standalone package.

Plan and implementation: validate both current take formats; feed one bounded
off-callback reader into a common timeline; add exact outputs and an explicit
stereo mix; wire browser, routes, transport, progress, faults and shutdown to the
40×13 shell; verify the actual adapter with fake JACK and the actual workflow
with null callbacks; update operating/engineering references; publish a scoped
commit and immutable peer receipt.

Acceptance checklist:

- [x] Native raw and SHR format-1 discovery/open, channel/metadata validation,
  meaningful errors, read-only source handling and damaged-take confirmation.
- [x] Bounded streaming and common play/pause/stop/seek; deterministic whole-period
  underrun/read faults and exact EOF tail; rate mismatch refusal.
- [x] Explicit exact output selection; stereo level/pan/mute, fixed 0.5/N gain,
  unity master, cancellable edits and private route saving.
- [x] Actual standalone controls/progress/error details at 40×13, common Session
  transport, failure cleanup, exit/signal ownership and reopen.
- [x] Synthetic native capture/publication/recovery/player path, distinguishable
  18/64-channel samples, bounded streaming, no-allocation callback checks,
  fake-JACK exact graph/startup/fault cleanup and null-device touch workflow.
- [x] Final normal production, formatting, check, Clippy and release gates.
- [x] Pi5 review of the published diff and immutable receipt (software accepted
  at dd5e2bab501580e886788c8ac8ab59b6989b516e; 48 normal tests independently
  rerun; private exchange `nodes/rpi5/0028-rec-accepted-20261112T141715Z-dd5e2ba.md`).
- [ ] Separately authorized native 18-channel sustained capture/playback,
  physical routing, storage load, listening and whole-device performance.

Evidence (rpi4, 2026-11-12): Rust 1.97.1
`8bab26f4f68e0e26f0bb7960be334d5b520ea452`, locked dependencies, one job,
CARGO_INCREMENTAL=0, canonical target, shared nonblocking build lock held by the
build parent throughout the sequence:

| Gate | Command | Result |
|---|---|---|
| Formatting | `cargo fmt --all -- --check` | PASS |
| All-target check | `cargo check --locked --all-targets` | PASS |
| Normal production | `cargo test --locked --all-targets -- --test-threads=1` | 48 passed, 0 failed, 0 ignored |
| Lint | `cargo clippy --locked --all-targets -- -D warnings` | PASS |
| Release | `cargo build --locked --release` | PASS (64 s) |
| Boundary | Release `--help`, `--version`, exported ABI symbols, dynamic libraries | PASS; same ten recorder C exports, no missing libraries |

The final source/test/Cargo/toolchain input digest is
`3a508941908f2ff9584977643c3c8196e3dc9672de7d2294466953eadba7867c`.
The first complete run caught a new PTY test's assumption that rendered words
were contiguous despite cursor-position escapes; the readiness predicate was
corrected, then all final gates above passed on unchanged production code.
Focused playback/adapter/recovery/workflow regressions also passed during work.
Historical/exhaustive, long-soak and physical-device classes were intentionally
not run. No JACK server was contacted by tests; no hardware/audio/MIDI action,
network load, service change or actual recording took place.

Synthetic fixtures are reproducible test source, created/removed in temporary
storage; no rendered audio or duplicate build trees are retained. Normal build
outputs (about 553 MiB) and the release binary/library remain for review/use.
The disposable newly generated zk index was removed after closing its handles;
notes/config/template remain. Exact artifact hashes and publication revision are
in the immutable Task0028 rpi4 ready-for-review receipt.

Next owner/action: Pi5 accepted this playback delivery for software on November 12.
Hardware gates remain unreserved. SP-04 owns the subsequent standalone capture
package; stopped-input meters, stereo capture groups and ALSA controller input
remain in stages 2/4.


## SP-04 — Task0029 standalone capture

Owner: the one authorized Pi4 worker, canonical main at the assigned
`dd5e2bab501580e886788c8ac8ab59b6989b516e` base; Pi5 reviews the actual diff and
performs final multi-repository synchronization. Source scope is REC only.
[Capture reference](CAPTURE.md) owns lifecycle/format details and commands;
[operating guide](LOCAL_SETUP.md) owns the complete touch path. Shared GP-GATE
integration stays in GigPies; there is no new GigPies runtime capture ABI.

Plan and implementation: preserve the existing host raw writer and ABI; add
no-start exact JACK capture and bounded interleaving; prepare the writer before
attachment/enablement; wire private source/destination setup and Session ownership;
quiesce before consuming the writer on a finalization worker; expose authoritative
completion, progress/faults and evidence-preserving deliberate copy recovery; test
the actual adapter with fake JACK and the complete UI with null boundaries.

Acceptance checklist:

- [x] Exact supported input names/UUIDs/rate inspection, cancellable source and
  destination selection, arm/status/limits, private SAVE, no implicit routes.
- [x] Startup writer readiness and successful attachment precede capture;
  synchronized raw stems, bounded preallocation and callback allocation guards.
- [x] Progress/limits, startup/refused/replaced-port, queue, rate, xrun/device and
  writer/publication failures remain explicit and incomplete where applicable.
- [x] Stop quiesces the producer before asynchronous drain/sync/publication;
  completion is authoritative, repeated stop does not create another finalizer.
- [x] Complete touch setup → capture → finalize → discover → existing player
  routes/prepare/render, stop/reopen another take, interrupted-copy recovery.
- [x] Deliberate recovery validates all stems/optional identities before creating
  output, preserves original evidence, and remains visibly recovered-incomplete.
- [x] Exclusive Session ownership, exit cancel/finalize, signal-style shutdown,
  error/drop cleanup, retry and retired-observer regression coverage.
- [x] Final normal production, formatting, check, Clippy and release gates.
- [x] Pi5 independent diff review and software acceptance of `24ff3ad6579b052d4faaffca910a819944330089`; all 58 normal tests independently rerun on Pi4 after ownership release.
- [ ] Separately authorized sustained native 18-channel capture/playback,
  physical routing/listening, storage load and device-performance acceptance.

Validation evidence and final source revision are recorded at closeout below.
No hardware/resource reservation, server/device action, MIDI or service changes
are authorized by this software delivery. Stopped-input meters, stereo capture
grouping and MIDI/controller expansion remain outside Task0029.


Validation (rpi4, 2026-11-12): exact Rust 1.97.1
`8bab26f4f68e0e26f0bb7960be334d5b520ea452`, locked dependencies, one job,
CARGO_INCREMENTAL=0, canonical target and the node’s parent-held shared build lock.

| Gate | Command | Result |
|---|---|---|
| Focused capture/adapter | `cargo test --locked --test capture_ui --lib -- --test-threads=1` | 23 passed |
| Normal production | `cargo test --locked --all-targets -- --test-threads=1` | 58 passed, 0 failed, 0 ignored |
| Formatting / check | `cargo fmt --all -- --check`; `cargo check --locked --all-targets` | PASS |
| Lint | `cargo clippy --locked --all-targets -- -D warnings` | PASS |
| Release | `cargo build --locked --release` | PASS (73 s) |
| Boundary | Release `--help`, `--version`, `nm -D`, `ldd` | PASS; same ten C exports; no missing libraries |

Session, header, Cargo.lock/Cargo.toml and toolchain inputs are unchanged. The raw
engine diff adds only an internal optional-metadata recovery hook; its public
raw/C recovery supplies no metadata and retains the existing behavior. Prior Pi5
playback acceptance is recorded in SP-03; the unchanged playback regressions pass.
A timing-dependent attempt to provoke queue overflow was replaced by deterministic
refused-push coverage; raw stopped-consumer overflow coverage remains normal.
The 18-channel touch regression explicitly selects exact output mode before
assigning outputs, preserving the existing deliberate playback workflow.

Historical/exhaustive, long-soak and physical-device test classes were not run.
No actual JACK server was contacted; no physical audio/MIDI, storage/load, service
or device action occurred. Removed two failed-test scratch trees and the newly
created disposable zk index after checking processes/open files: 10,760,192 allocated
bytes reclaimed (10,354,688 in tmpfs, 405,504 on disk; 10,365,698 apparent bytes).
Successful synthetic fixtures self-clean. Normal target output is about 590 MiB;
release executable/library and reproducible source/tests are retained. No scratch
runner, fixture audio, duplicate workspace or generated evidence inventory remains.

Pi5 acceptance (November 12): actual source diff and receipt reviewed; callback
ownership, startup, durable completion, recovery and Session exclusivity inspected.
The coordinator independently reran all 58 normal tests on Pi4 at the published
implementation above. All passed. CI passed on x86_64/ARM including Required CI.
This acceptance update changes documentation only and reuses those unchanged
source/test/lock/toolchain inputs. The Task0029 exchange ledger owns final exact
revision/hash verification and thirteen-repository synchronization. All physical
gates remain unreserved and unperformed.
