# SHR REC: GigPies lifecycle implementation

Planning baseline **2026-10-04 / GP-2026-10-04.1**. REC-01/REC-02 are now implemented in task0009; remaining tasks are deferred. [Central inventory](https://github.com/PaolaShultz/gigpies/blob/main/docs/MODULE_IMPLEMENTATION_MAP.md) ·
[Agreed contracts](https://github.com/PaolaShultz/gigpies/blob/main/docs/MODULE_CONTRACTS.md). Existing product roadmaps remain authoritative for
unrelated work; this plan owns only the GigPies integration increments below.

## Objective and boundary

Own bounded recording worker, sample integrity and recovery; Stagebox owns physical capture and tap identity. Provide recorder status/control seam without enabling standalone playback/UI by implication. Keep raw v1 format and C ABI stable; do not create a second capture owner.

## Source and evidence reviewed

Repository: `/home/shome/p/shr-rec`. Inspected HEAD: `c0416d286eef2a455201dce4e992d9b3f6418f52`.
Clean at inspection; recheck before editing. This dated observation is not a future ownership claim.

Owning documents: README.md; docs/STATUS.md, PLAN.md, ARCHITECTURE.md, RAW_RECORDER.md; include/shr_rec.h.

Source inspected: `src/session.rs::{Session,Transport,Effect}`, `src/recorder.rs::{Recorder,Report,recover}`, C symbols, `src/config.rs`.

`Recorder::{create,push,finish}` and C v1 are implemented with bounded queues,
PCM24, journal, no-replace finalization and evidence-preserving recovery. The
shell `Session` has no operation generation and its engine effects are unwired.
Twenty-three normal tests and H8 integrated raw-library evidence are recorded.
Standalone source schema/playback remain planned; received/queued frames are not
proof of committed stems or hardware continuity.

These are source inspection and previously recorded results, not fresh builds or
physical acceptance. The planning session runs documentation checks only.

## Milestones and tasks

First: operation-scoped lifecycle prevents stale readiness/completion from reviving retired takes. Next: safe worker progress query and synthetic host adapter; then full standalone route/take/player work under existing PLAN.md when separately selected.

Task states are execution dependencies: READY has no missing software provider;
WAITING names its precise prerequisite; DEFERRED has an activation condition.
Source delivery and build reservation are additional launch prerequisites on a
peer. Every row has one owner, the repository named in its Owner column. A later
task starts only after the previous artifact is reviewed, never merely delivered.

| Task / priority / state | Owner | Work area, inputs and required artifact | Output and measurable acceptance |
|---|---|---|---|
| REC-01 / P1 / IMPLEMENTED | SHR REC | `src/session.rs`, session tests; C-REC:1/E06. | Add operation identity to asynchronous lifecycle events; explicit accepted vs ready/finalized state; stop-during-start, stale ready, duplicate stop and old completion tests. Keep shell unavailable until actual adapter exists. Publish E06 state-vector corpus. |
| REC-02 / P1 / IMPLEMENTED | SHR REC | REC-01; C-REC:1 counters/lifecycle, raw v1. `src/recorder.rs`, include/shr_rec.h, raw docs/tests. | Add bounded non-owning progress snapshot/query with a new versioned additive symbol, accepted vs written vs durable-unknown, first fault and finish outcome. Do not traverse mutable worker state or alter v1 signatures. Synthetic host proves quiescence before one consuming finish; GP-05 consumes header/fixtures/library hashes. Strong lifetime review before ABI write. |
| REC-03 / P2 / DEFERRED | SHR REC | Explicit standalone milestone selection in PLAN.md after REC-02. | Native take/source/stereo schema, then synchronized player, then shell/JACK route wiring as separate bounded tasks; never call raw-v1 identical to DAW format 1. No GigPies requirement to finish player first. |
| REC-H1 / P2 / DEFERRED | SHR REC | GP-H1/H3 or separate authorized standalone/storage gate. | Native multichannel throughput, power-loss/storage recovery and playback route acceptance. Pi4 synthetic tests do not certify NVMe throughput. |

## Validation and failure behavior

Focused `CARGO_INCREMENTAL=0 cargo +1.97.1 test --locked --lib session -j 1`, full `cargo +1.97.1 test --locked --all-targets -j 1` under same environment after shared lifecycle changes. fmt; warning-denied all-target Clippy. Shell changes additionally release build and `python3 tests/terminal.py target/release/shr-rec`. Retain existing real-child crash and bounded disk-error regressions in normal suite; no long disk soak.

Preserve whole-period push/drop, same frame counts, no false complete on flush/fsync failure, no arbitrary remote path, no source mutation during recovery, symlink/live-writer refusal. Callback progress publication must not allocate/lock/I/O; finish remains off-thread. Brain/UI loss must not stop capture.

Historical research, auditions, exhaustive matrices, long soaks, full-show renders
and physical/combined-load checks are intentionally outside the normal software
milestones unless their protected behavior changes. Retain their owning documented
on-demand commands; no private media download or test hardware side effect.
Independent builds retain lockfiles and existing repository editions; this plan
does not upgrade dependencies/editions or replace existing intra-repository workspace
paths. The ban is on new sibling-repository path dependencies.

## Resources, review and recovery of work

Wave 2 local rpi5 after first local review; alternatively transfer this exact owner revision to a freed Pi4 lane for pure lifecycle tests. First task <768 MiB compiler RSS/≤512 MiB growth estimate; jobs=1 host slot. Synthetic temporary recordings ≤32 MiB total retained only through assertions. NVMe-specific work stays on suitable hardware.

Independent fallback: REC-01 adversarial state-vector/recovery review without compiling. No unbounded render or research assignment.
Before builds check free space and target size; below 20 GiB free or above 5 GiB
output is a review, not permission to delete another task's cache. No reduced
coverage/debug information to make a budget appear to pass.

Handoff: exact changed files, commit plus patch hashes or bounded source manifest
if uncommitted, contract IDs/versions and provider-fixture hashes, commands/results,
intentional skipped classes, remaining limits and next task/owner. Stage only named
owned changes if a later implementation session commits; no public push is implied.
Receiving owner reviews independently and writes an immutable private-ledger
acknowledgement. Interrupted work stays visible with last completed acceptance
criterion; never reset/stash/clean another session or replay an uncertain mutation.


## Implementation launch prompt

Host/cwd assignments and source preparation are in GigPies PARALLEL_WORK_PLAN.md.
This historical first-task prompt was superseded by task0009 continuation.
Its ownership/safety constraints remain relevant; its stopping point does not.

```text
Work only in the current shr-rec checkout. Read AGENTS.md (if present), the
owning docs and docs/GIGPIES_IMPLEMENTATION.md, then the referenced GP-2026-10-04.1 contracts.
Implement only REC-01; keep progress and evidence in this plan. Check hostname,
HEAD/source manifest, live Git state, active ownership and current contract hashes
before edits; preserve other sessions and unrelated work. If this is a delivered
snapshot, verify its handoff manifest and separate receiving acknowledgement first.
Reserve this host's one build slot as described in GigPies PARALLEL_WORK_PLAN.md;
use Rust 1.97.1, Cargo.lock, CARGO_INCREMENTAL=0 and cargo -j 1 in normal target/.
Run focused checks during work and the required normal suite for changed behavior.
No sibling writes, sibling path dependencies or unilateral contract changes.
No audio/MIDI/DMX/playback/device/display/service changes or shared load tests.
When blocked report exact provider/task/version mismatch and continue only the
independent fallback REC-01 adversarial state-vector/recovery review without compiling within this repository; do not fake acceptance.
Stop after the scoped task and reviewable handoff, before later milestones,
physical operations, publication or deployment. Do not stage unrelated files or
claim mock, planned or incomplete behavior is a finished engine.
```

## Progress

- 2026-10-04: source and owner documents inspected; plan written. Implementation
  tasks remain in the states above. Physical evidence retains its original limits.

### Task0009 REC software delivery — 2026-10-04

REC-01 and REC-02 implemented within the assigned repository. Session-generated
operation IDs correlate preparation/finalize effects and readiness/completion;
Stop while preparing retires readiness and waits for cleanup acknowledgment.
The v2 observer uses an independent Arc atomic allocation with off-callback
create/retain/release, exact output-size checking and terminal outcome after
join on finish/drop/error/panic. No v1 signature or raw format changed.

E06 source corpus and direct C runner are in `tests/fixtures/crec/v1`.
Shell remains unavailable for recording/playback; standalone adapter/player and
physical/load gates remain deferred. Central C-REC additions are proposed to
root in the private checkpoint; this worker does not edit MODULE_CONTRACTS.

Task0009 validation (with the [shared parent-held build lock](https://github.com/PaolaShultz/gigpies/blob/main/docs/PARALLEL_WORK_PLAN.md#one-build-slot-per-host), one job and `CARGO_INCREMENTAL=0`):

```sh
cargo +1.97.1 fmt --check
cargo +1.97.1 test --locked -j1 --all-targets
cargo +1.97.1 clippy --locked -j1 --all-targets -- -D warnings
cargo +1.97.1 build --locked -j1 --release
```

Full normal suite: **30 tests pass**, including isolated shell PTY, real-child
crash/copy recovery, operation corpus, actual preparing-writer cleanup,
concurrent observer, direct Rust ABI, sample integrity and callback allocation
checks. Formatting, Clippy and release pass. No native/host feature gates exist
in this crate. Hardware, historical media, exhaustive/long soak and shared-load
checks intentionally skipped; no endpoints were discovered/opened.

The linked C11 runner (`-Wall -Wextra -Werror`) also passed against the release
shared library:96 exact PCM24 frames/stem, observer lifetime/bounds, complete
final outcome. Release PTY `python3 tests/terminal.py target/release/shr-rec`
and final formatting passed. Temporary PCM and runner removed; concise artifact
hashes and reproducible commands retained in task0009 private B checkpoint.
No source commits/pushes were performed by this worker; root owns publication.
