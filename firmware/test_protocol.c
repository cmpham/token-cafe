// Host-side memory-safety stress test; complements real Xtensa execution in QEMU.
#include "main/protocol.h"
#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

static unsigned replies;
static uint8_t status;
static void write_reply(const uint8_t *data, size_t size) {
    assert(size >= 17 && size <= 32);
    assert(memcmp(data, "TCF1", 4) == 0);
    status = data[16];
    ++replies;
}
static bool fake_input(uint8_t out[5]) {
    const uint8_t event[5] = {1, 80, 0, 16, 0};
    memcpy(out,event,5);return true;
}
static void present(const uint8_t *frame) { assert(frame); }

static void rle_request_op(tc_protocol *p, const uint8_t *body, size_t size, uint8_t op) {
    uint8_t msg[64] = {'T','C','F','1',6,1};
    assert(size <= sizeof(msg) - TC_HEADER);
    msg[4] = op;
    msg[6] = size;
    memcpy(msg + TC_HEADER, body, size);
    uint32_t crc = tc_crc32(tc_crc32(0, msg, 12), body, size);
    for (int i = 0; i < 4; i++) msg[12+i] = crc >> (8*i);
    tc_feed(p, msg, TC_HEADER + size);
}
static void rle_request(tc_protocol *p, const uint8_t *body, size_t size) { rle_request_op(p, body, size, 6); }
int main(int argc, char **argv) {
    uint8_t *frame = malloc(TC_FRAME_BYTES);
    tc_protocol *p = malloc(sizeof(*p));
    assert(frame && p);
    tc_init(p, frame, write_reply, present);
    assert(tc_crc32(0, (const uint8_t *)"123456789", 9) == 0xcbf43926);
    // Full-panel edges: accept all visible corners and reject out-of-panel input.
    const uint16_t taps[][3] = {
        {0, 0, 0}, {479, 271, 0}, {0, 271, 0}, {479, 0, 0},
        {480, 0, 1}, {0, 272, 1}, {65535, 0, 1}, {0, 65535, 1},
    };
    for (size_t i = 0; i < sizeof(taps) / sizeof(taps[0]); ++i) {
        uint8_t tap[] = {taps[i][0] & 255, taps[i][0] >> 8,
                         taps[i][1] & 255, taps[i][1] >> 8};
        rle_request_op(p, tap, sizeof(tap), 4);
        assert(status == taps[i][2]);
    }
    puts("PASS: 480x272 full-panel touch corners and out-of-bounds rejection");
    rle_request_op(p, (const uint8_t *)"", 0, 9);assert(status==0);
    p->input=fake_input;
    rle_request_op(p, (const uint8_t *)"", 0, 9);assert(status==0);
    rle_request_op(p, (const uint8_t *)"x", 1, 9);assert(status==1);
    p->input=NULL;
    puts("PASS: optional physical-input polling and invalid payload rejection");
    // Invalid LZ4 blocks must not partially mutate a rectangle.
    uint8_t compressed[]={0,0,0,0,TC_WIDTH&255,TC_WIDTH>>8,1,0,0x10,0xff,0,0};
    rle_request_op(p,compressed,sizeof(compressed),10);assert(status==1);
    for(size_t i=0;i<TC_FRAME_BYTES;i++) assert(frame[i]==0);
    for(size_t n=8;n<sizeof(compressed);n++) {
        rle_request_op(p,compressed,n,10);assert(status==1);
        for(size_t i=0;i<TC_FRAME_BYTES;i++) assert(frame[i]==0);
    }
    puts("PASS: LZ4 invalid offsets and truncation rejected atomically");
    // Overlapping LZ4 copy expands one literal to a complete display row.
    uint8_t valid_lz4[]={0,0,0,0,TC_WIDTH&255,TC_WIDTH>>8,1,0,
                        0x1f,0xab,1,0,255,255,255,170,0x50,0xab,0xab,0xab,0xab,0xab};
    rle_request_op(p,valid_lz4,sizeof(valid_lz4),10);assert(status==0);
    for(size_t i=0;i<TC_WIDTH*2;i++) assert(frame[i]==0xab);
    uint32_t compressed_crc=tc_crc32(0,frame,TC_FRAME_BYTES);
    valid_lz4[sizeof(valid_lz4)-6]=0x60; // Invalid trailing literal after a valid match.
    rle_request_op(p,valid_lz4,sizeof(valid_lz4),10);assert(status==1);
    assert(tc_crc32(0,frame,TC_FRAME_BYTES)==compressed_crc);
    memset(frame,0,TC_FRAME_BYTES);
    puts("PASS: overlapping LZ4 copies and late-error atomicity");
    // Every possible length, followed by arbitrary payload fragments. Sanitizers
    // detect out-of-bounds accesses; invalid packets must leave the frame alone.
    uint8_t header[16] = {'T','C','F','1',2,1};
    uint8_t junk[257];
    uint32_t random = 17;
    for (unsigned len = 0; len <= 65535; ++len) {
        tc_reset(p);
        header[6] = len; header[7] = len >> 8;
        tc_feed(p, header, sizeof(header));
        for (size_t i = 0; i < sizeof(junk); ++i) {
            random = random * 1664525u + 1013904223u;
            junk[i] = random >> 24;
        }
        tc_feed(p, junk, sizeof(junk));
    }
    for (size_t i = 0; i < TC_FRAME_BYTES; ++i) assert(frame[i] == 0);
    assert(replies > 60000);
    // Multi-row RLE, zero/oversized/undersized runs, and no partial mutation.
    uint8_t body[] = {1,0,1,0,2,0,2,0, 4,0,0x34,0x12};
    tc_reset(p);
    rle_request(p, body, sizeof(body));
    assert(status == 0);
    assert(frame[(TC_WIDTH + 1)*2] == 0x34 && frame[(TC_WIDTH*2 + 2)*2+1] == 0x12);
    uint32_t baseline = tc_crc32(0, frame, TC_FRAME_BYTES);
    for (int count = 0; count <= 5; count++) {
        if (count == 4) continue;
        body[8] = count;
        rle_request(p, body, sizeof(body));
        assert(status == 1 && tc_crc32(0, frame, TC_FRAME_BYTES) == baseline);
    }
    uint8_t late_bad[] = {1,0,1,0,2,0,2,0, 2,0,0xff,0xff, 3,0,0xff,0xff};
    rle_request(p, late_bad, sizeof(late_bad));
    assert(status == 1 && tc_crc32(0, frame, TC_FRAME_BYTES) == baseline);
    body[8] = 4;
    rle_request_op(p, body, sizeof(body), 8);
    assert(status == 0);
    for (size_t i = 0; i < TC_FRAME_BYTES; ++i) assert(frame[i] == 0);
    puts("PASS: XOR delta round trip");
    puts("PASS: RLE multi-row decode and atomic rejection of invalid runs");
    // Cross-language fixtures generated by the Rust transport encoder.
    if(argc==3) {
        FILE *stream=fopen(argv[1],"rb"), *expected=fopen(argv[2],"rb");
        assert(stream && expected);
        tc_init(p,frame,write_reply,present);
        p->scratch=malloc(TC_FRAME_BYTES);assert(p->scratch);
        uint8_t chunk[17];size_t n;
        while((n=fread(chunk,1,sizeof(chunk),stream))) { tc_feed(p,chunk,n);assert(status==0); }
        for(size_t i=0;i<TC_FRAME_BYTES;i++) assert(fgetc(expected)==frame[i]);
        assert(fgetc(expected)==EOF);
        fclose(stream);fclose(expected);free(p->scratch);
        puts("PASS: Rust encoder to C decoder exact pixel equality");
    }
    free(p); free(frame);
    puts("PASS: CRC known vector and all 65,536 payload lengths under sanitizers");
}
