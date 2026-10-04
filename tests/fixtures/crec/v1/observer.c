/* Synthetic direct ABI runner: no physical endpoints. Directory must be new. */
#include "shr_rec.h"
#include <assert.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
_Static_assert(sizeof(shr_rec_progress_v2) == 96, "v2 ABI size");
_Static_assert(offsetof(shr_rec_progress_v2, epoch) == 24, "v2 ABI offset");
static void verify(const char *root, int channel) {
    char path[4096];
    assert(snprintf(path, sizeof path, "%s/channel-%03d.wav", root, channel) > 0);
    FILE *f = fopen(path, "rb"); assert(f);
    unsigned char header[44]; assert(fread(header, 1, 44, f) == 44);
    assert(memcmp(header, "RIFF", 4) == 0 && header[40] == 32 && header[41] == 1);
    for (int frame = 0; frame < 96; ++frame) {
        int code = (frame % 48 + 1) * (channel == 1 ? 1 : -1);
        unsigned char data[3]; assert(fread(data, 1, 3, f) == 3);
        unsigned int expected = (unsigned int)code;
        for (int b = 0; b < 3; ++b) assert(data[b] == ((expected >> (b*8)) & 255));
    }
    assert(fgetc(f) == EOF); assert(fclose(f) == 0);
}
int main(int argc, char **argv) {
    assert(argc == 2);
    shr_rec_handle *rec = shr_rec_v1_create(argv[1], 48000, 2, 48, 8, 9); assert(rec);
    shr_rec_observer *obs = shr_rec_v2_observer_create(rec); assert(obs);
    shr_rec_observer *retained = shr_rec_v2_observer_retain(obs); assert(retained);
    shr_rec_progress_v2 status; memset(&status, 0x5a, sizeof status);
    assert(shr_rec_v2_observer_snapshot(obs, &status, sizeof status - 1) == -1);
    assert(status.version == 0x5a5a5a5a);
    assert(shr_rec_v2_observer_snapshot(NULL, &status, sizeof status) == -1);
    assert(status.version == 0x5a5a5a5a);
    assert(shr_rec_v2_observer_snapshot(obs, NULL, sizeof status) == -1);
    double samples[96];
    for (int f = 0; f < 48; ++f) {samples[2*f] = (f+1)/8388608.0; samples[2*f+1] = -(f+1)/8388608.0;}
    for (int b = 0; b < 2; ++b) {
        assert(shr_rec_v1_push(rec, 48048 + b*48, samples, 48) == 0);
        assert(shr_rec_v2_observer_snapshot(obs, &status, sizeof status) == 0);
        assert(status.written_frames <= status.accepted_frames && status.outcome == 0);
    }
    shr_rec_v2_observer_release(obs);
    assert(shr_rec_v1_finish(rec) == 0);
    assert(shr_rec_v2_observer_snapshot(retained, &status, sizeof status) == 0);
    assert(status.version == 2 && status.state == 3 && status.outcome == 1 && status.epoch == 9);
    assert(status.accepted_frames == 96 && status.written_frames == 96 && status.durable_frames == UINT64_MAX);
    assert(status.reserved == 0 && status.writer_fault == 0 && status.host_fault == 0);
    shr_rec_v2_observer_release(retained);
    verify(argv[1], 1); verify(argv[1], 2);
    puts("E06 direct C ABI: 96 exact frames/stem, finalized complete, observer lifetime/bounds pass");
    return 0;
}
