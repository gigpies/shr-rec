# Standalone capture boundary

[Operating guide](LOCAL_SETUP.md) · [Raw writer / C ABI](RAW_RECORDER.md)

The standalone application owns a no-start `shr-rec-capture` JACK client. REFRESH
opens a short-lived discovery client and lists supported JACK float-mono audio
output ports, exact names and current UUIDs. It creates no connections. ARM and
configuration inspection do not open a device. RECORD is the explicit connection
request: only selected exact sources connect to this app's capture inputs. There
are no capture outputs, dry-monitoring path, server startup or server changes.
Playback has its separate client and is excluded by the common Session owner.

REFRESH never rewrites remembered names. RECORD validates every armed assignment
and snapshots its current UUID/rate. Missing/ambiguous names block preparation.
The adapter rechecks rate and UUID through discovery, registration and connection;
a port replaced under the same name refuses attachment. Deliberate REFRESH and
inspection accept a replacement for a later attempt. Mid-take exact-edge removal,
replacement, rate change, xrun or server shutdown disables capture and requests
whole-timeline finalization. An unrelated graph change does not stop capture.

## Ownership and completion

The owner accepts a correlated Session operation, then a preparation worker opens
a new private take and waits for the raw writer's startup handshake. It writes
and synchronizes the identity sidecar before returning readiness. The owner
registers/activates JACK with the producer disabled, connects all selected edges,
checks startup faults, accepts Session readiness, and only then enables capture.
Preparation failure never enables the producer. Stop during preparation retires
readiness and finalizes the prepared writer without attaching it.

One callback exclusively owns the Recorder inside stable UnsafeCell storage.
It interleaves at most 8192 frames × 64 channels into preallocated f64
scratch and submits one whole period to the existing SPSC engine. It neither
allocates/frees nor locks, accesses files, logs, formats, spawns or waits. There
is no second metering/capture owner. Independent atomic peak snapshots drive the
recording screen’s 18 circular LED overview; the bank follows selected input
(1–18, 19–36, and so on). Meter colors indicate silence/low, green, yellow at
0.5 peak and red at 0.99; metering does not change recorded gain.
At most 16 queued periods are accepted;
raw sample storage is bounded to 64 MiB at the 64-channel ceiling (18 channels
use 18 MiB), plus fixed scratch/descriptor buffers. Queue-full, invalid sample,
oversize period and writer errors stop rather than lose individual stem frames.
This is a bound, not a sustained-channel performance claim.

STOP disables capture, deactivates/closes only the owned JACK client and waits
for its callback lifetime barrier before moving the Recorder out. A finalization
worker consumes it once, drains accepted periods, finalizes and synchronizes
stems and publishes the authoritative result without replacement. Session stays
Finalizing and refuses both transports until that worker returns. Repeated STOP
starts no extra writer/finalizer. Retained progress comes only from the independent
atomic observer: A = accepted frames, W = written frames; neither means durable
completion. Only successful `finish` with a complete report permits Complete.
Empty, clipped, gapped or faulted takes remain incomplete. A failure retains its
path, partial stems, timeline and any failure report for explicit inspection.

The RIFF ceiling is `(UINT32_MAX - 37) / 3` mono frames; the screen shows its time
at the inspected rate and the armed payload rate before RECORD. The writer checks
a 16 MiB storage reserve at startup/every 256 blocks and checks every I/O and sync.
There is no segmentation. Concurrent storage consumers can still exhaust space.
An OS failure to create a finalization thread falls back to draining the writer
on the owner thread and reports incomplete. Ordinary finalization is asynchronous;
shutdown can wait for in-flight regular-file I/O.

## Configuration and native format

`DATA_ROOT/capture.json` is a version-1 private preference file, separate from
initial `config.json` and playback preferences. SAVE retains ordered stable track
IDs, labels, arm states, preferred exact source names and an absolute destination.
The existing config version remains unchanged. SAVE uses a mode-0600 create-new
`.capture.json.part`, sync, rename and directory sync. A stale temporary blocks
SAVE for inspection; loading rejects nonregular/final-component symlink, oversized
or unsupported preference files. Loading alone never creates files or attaches
JACK. An explicit `--takes-root` overrides the remembered destination for this run;
otherwise the default is DATA_ROOT/takes. DEST browses existing directories with
bounded direct-child listings; CANCEL does not change the current destination.

The native raw v1 manifest, PCM24 mono stems, timeline, integrity counters and
result are unchanged. Standalone takes add an optional version-1 `capture.json`
inside the take with the ordered armed track records and actual selected JACK
UUIDs. Successful attachment requires each preferred name to equal its actual
port; there is no substitution. The player validates the sidecar, uses its stable
IDs/labels and keeps physical input assignment separate from output assignment.
Older raw takes without it retain their generic ordered channel identities.
No new C export or header change is required. Stereo capture grouping, input
meters while stopped and controller expansion remain future work.

RECOVER → COPY is an explicit owner-thread request to copy recovery on a worker.
It validates all source stems and optional identity metadata before creating the
new destination. It takes the raw recovery source lock, refuses a live writer,
symlink/ambiguous/malformed/newer input, and never repairs/truncates originals.
Partial stem publication is accepted only when exactly one final/temporary name
exists for each stem. Recovery preserves the identity sidecar when present and
retains only common valid frames; source evidence/timeline stays available for
review. The new take remains recovered-incomplete and requires the player's
existing deliberate ACCEPT. A failed recovery leaves all source evidence intact
and any newly created partial destination visible; retry uses a new name.

## Shutdown and offline validation

EXIT while Starting/Recording offers Stop and finalize / Cancel. Cancellation
leaves the producer running. Confirmation waits for completion; incomplete/error
results remain visible before another deliberate EXIT. Finalizing allows an exit
request but never skips the writer. SIGINT/SIGTERM/SIGHUP and terminal read/draw
errors use quiescence/finalization and terminal restoration; the retained result
is printed after leaving the alternate screen. Dropping an active owner also
quiesces/joins and marks its take incomplete. SIGKILL/power loss cannot clean up;
use explicit copy recovery after restart.

Focused production commands (under the node's shared build lock):

```sh
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo test --locked --test capture_ui --lib -- --test-threads=1
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo test --locked --all-targets -- --test-threads=1
```

Fake JACK tests exercise the actual attachment/callback/notification/teardown
code, every installation/activation/connection failure, startup disabled capture,
rate/port-identity refusal, device/edge/xrun/period faults and allocation/free
guards. Null-device touch tests exercise setup → capture → finalize → discover →
player routes/prepare/render → stop/reopen, interruption/copy recovery, cancel
exit, startup/publication failures, retry and shutdown. Existing raw tests cover
write/space/finalization failure, crash/partial-publication recovery, integrity,
ABI and observer contracts. The touch workflow carries 18 distinguishable
channels through capture/player routing; playback additionally covers 64 channels.
No test contacts a real JACK server. Sustained 18-channel recording/playback,
physical routing/listening, real storage load, power-loss and device performance
remain separately authorized acceptance gates. MIDI/controllers are excluded.
