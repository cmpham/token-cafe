#pragma once
#include <stddef.h>
#include <stdbool.h>
#include <stdint.h>

#define TC_WIDTH 480
#define TC_HEIGHT 272
#define TC_SCENE_WIDTH 480
#define TC_SCENE_HEIGHT 272
#define TC_FRAME_BYTES (TC_WIDTH * TC_HEIGHT * 2)
#define TC_MAX_PAYLOAD 4096
#define TC_HEADER 16

typedef void (*tc_write_fn)(const uint8_t *, size_t);
typedef void (*tc_present_fn)(const uint8_t *);
typedef struct {
    uint8_t packet[TC_HEADER + TC_MAX_PAYLOAD];
    size_t used;
    uint8_t *frame;
    uint8_t *scratch; // Optional TC_FRAME_BYTES buffer for compressed XOR.
    tc_write_fn write;
    tc_present_fn present;
    bool (*input)(uint8_t out[5]);
    void (*restore)(uint8_t *frame); // Restore last committed frame on preemption.
} tc_protocol;

uint32_t tc_crc32(uint32_t crc, const uint8_t *data, size_t size);
void tc_init(tc_protocol *p, uint8_t *frame, tc_write_fn write, tc_present_fn present);
void tc_feed(tc_protocol *p, const uint8_t *data, size_t size);
// Discard a partial message after a transport idle timeout/reconnect.
void tc_reset(tc_protocol *p);
