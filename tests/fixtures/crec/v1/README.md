# C-REC:1 / E06 owner corpus

`e06.json` is executed against the real Session by the normal contract suite.
It uses operation 17 after retiring operations 1..16 and tests acceptance,
readiness, stale operation events, duplicate Start/Stop and exactly one completion.
The preparing-writer regression additionally cleans up a real Recorder whose
producer was never activated before accepting a retry.

`observer.c` is a direct linked C runner against `include/shr_rec.h` and the
release library. It invokes v1 create/push/finish, v2 observer retain/query/release,
checks layout/no-write size rejection and verifies every PCM24 code in two stems
(96 frames each at source frame 48048, epoch 9). See RAW_RECORDER.md for commands.
Normal Rust tests exercise the same direct symbols plus fault/drop/panic outcomes,
concurrent producer/observer queries and callback allocation checks.

Active 96 accepted / 48 written is a valid asynchronous state, not a deterministic
scheduler assertion. Finalized success requires 96 accepted / 96 written; durability
is unknown. Hosts own take/operation/source-map identities. Fixtures do not open
physical endpoints or certify storage power loss, timing, or hardware continuity.
Generated PCM, runner binaries and private artifact manifests are not source.
