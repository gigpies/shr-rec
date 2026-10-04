#ifndef SHR_REC_V1_H
#define SHR_REC_V1_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct shr_rec_handle shr_rec_handle;
/* Setup/control thread. Directory must not exist. NULL means startup failed;
 * retain any files created there. Only one producer may use a handle. */
shr_rec_handle *shr_rec_v1_create(const char *directory, uint32_t sample_rate,
    uint32_t channels, uint32_t max_block, uint32_t queue_blocks, uint64_t epoch);
/* Callback-safe: normalized frame-major interleaved doubles. 0 accepted,
 * 1 whole block dropped (queue full), -1 invalid, -2 writer failed. */
int32_t shr_rec_v1_push(shr_rec_handle *, uint64_t source_frame,
    const double *interleaved, uint32_t frames);
/* Callback-safe, nonzero host-defined xrun/disconnect/etc. code; first retained. */
void shr_rec_v1_mark_fault(shr_rec_handle *, uint32_t code);
/* Setup-only optional source precision declaration, 1..24; 0 success, -1 invalid.
 * This never infers converter precision from the output WAV container. */
int32_t shr_rec_v1_set_source_bits(shr_rec_handle *, uint32_t bits);
/* Quiesce producer first. Consumes handle, drains/joins/syncs writer. 0 complete,
 * 1 finalized incomplete, -1 finalization failed. Never call twice. */
int32_t shr_rec_v1_finish(shr_rec_handle *);
/* Control-thread recovery copies to a NEW destination, preserving source.
 * 1 recovered-incomplete, -1 refusal/error. No recovery ever returns complete. */
int32_t shr_rec_v1_recover(const char *source, const char *destination);
/* Additive version 2 observer; v1 recorder ownership/signatures unchanged.
 * Create while producer is quiesced, off callback. Observer owns independent
 * refcounted atomics, never a recorder pointer, and survives v1_finish.
 * Retain creates another independently releasable handle. Create/retain/release
 * allocate/free and MUST be off callback. Keep each handle alive during query;
 * release cannot race use. Recorder finish still requires producer quiescence.
 * Host owns take/operation IDs and ordered source mapping; epoch is immutable. */
typedef struct shr_rec_observer shr_rec_observer;
typedef struct shr_rec_progress_v2 {
    uint32_t version, state, outcome, writer_fault, host_fault, reserved;
    uint64_t epoch, accepted_frames, written_frames, durable_frames;
    uint64_t dropped_frames, overflow_blocks, gap_frames, invalid_blocks, clipped_samples;
} shr_rec_progress_v2;
/* state: 1 ready/recording, 2 finalizing, 3 finalized.
 * outcome: 0 pending, 1 complete, 2 incomplete, 3 error.
 * writer_fault: 0/1; host_fault: first nonzero host code; reserved: always zero.
 * durable_frames: UINT64_MAX (unknown), including after finish; no power-loss
 * certification. written counts whole stems + journal written, never fsync.
 * Active counters are individually monotonic, weakly consistent; written is
 * sampled before accepted. Final outcome Release/Acquire publishes counters.
 * Poll off callback at <=10Hz. Snapshot reads bounded atomics only.
 * Exact sizeof(shr_rec_progress_v2) required. -1 null/wrong size without writes;
 * 0 success, version=2. Output must be aligned, writable and unaliased. */
shr_rec_observer *shr_rec_v2_observer_create(shr_rec_handle *);
shr_rec_observer *shr_rec_v2_observer_retain(const shr_rec_observer *);
void shr_rec_v2_observer_release(shr_rec_observer *);
int32_t shr_rec_v2_observer_snapshot(const shr_rec_observer *,
    shr_rec_progress_v2 *, uint32_t output_size);
#ifdef __cplusplus
}
#endif
#endif
