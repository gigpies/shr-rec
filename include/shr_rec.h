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
#ifdef __cplusplus
}
#endif
#endif
