#include "protocol.h"
#include <string.h>

static uint16_t u16(const uint8_t *p) { return p[0] | ((uint16_t)p[1] << 8); }
static uint32_t u32(const uint8_t *p) { return u16(p) | ((uint32_t)u16(p + 2) << 16); }
static void put16(uint8_t *p, uint16_t v) { p[0] = v; p[1] = v >> 8; }
static void put32(uint8_t *p, uint32_t v) { put16(p, v); put16(p + 2, v >> 16); }

uint32_t tc_crc32(uint32_t crc, const uint8_t *data, size_t size) {
    crc = ~crc;
    for (size_t i = 0; i < size; ++i) {
        crc ^= data[i];
        for (int bit = 0; bit < 8; ++bit)
            crc = (crc >> 1) ^ (0xedb88320u & (0u - (crc & 1)));
    }
    return ~crc;
}

static void reply(tc_protocol *p, uint8_t status, const uint8_t *data, size_t size) {
    uint8_t out[TC_HEADER + 16] = {0};
    memcpy(out, "TCF1", 4);
    out[4] = p->packet[4] | 0x80;
    out[5] = 1;
    put16(out + 6, size + 1);
    memcpy(out + 8, p->packet + 8, 4);
    out[16] = status;
    if (size) memcpy(out + 17, data, size);
    put32(out + 12, tc_crc32(tc_crc32(0, out, 12), out + 16, size + 1));
    p->write(out, 17 + size);
}

static void dispatch(tc_protocol *p) {
    const uint8_t *data = p->packet + TC_HEADER;
    size_t len = u16(p->packet + 6);
    uint8_t result[8] = {0};
    if (tc_crc32(tc_crc32(0, p->packet, 12), data, len) != u32(p->packet + 12)) {
        reply(p, 2, NULL, 0); return;
    }
    switch (p->packet[4]) {
    case 1: // HELLO: dimensions and maximum payload, RGB565 little endian.
        if (len) break;
        put16(result, TC_WIDTH); put16(result + 2, TC_HEIGHT);
        put16(result + 4, TC_MAX_PAYLOAD); result[6] = 1;
        reply(p, 0, result, 7); return;
    case 2: { // RECT: validate the entire update before changing memory.
        if (len < 8) break;
        uint32_t x = u16(data), y = u16(data + 2);
        uint32_t w = u16(data + 4), h = u16(data + 6);
        if (!w || !h || x + w > TC_WIDTH || y + h > TC_HEIGHT ||
            len != 8 + w * h * 2) break;
        for (uint32_t row = 0; row < h; ++row)
            memcpy(p->frame + ((y + row) * TC_WIDTH + x) * 2, data + 8 + row * w * 2, w * 2);
        reply(p, 0, NULL, 0); return;
    }
    case 6:
    case 8: { // RLE rectangle (8 = XOR against staging pixels): (count:u16, RGB565:u16) runs, validated atomically.
        if (len < 12 || (len - 8) % 4) break;
        uint32_t x = u16(data), y = u16(data + 2);
        uint32_t w = u16(data + 4), h = u16(data + 6), total = 0;
        if (!w || !h || x + w > TC_WIDTH || y + h > TC_HEIGHT) break;
        for (size_t i = 8; i < len; i += 4) {
            uint32_t count = u16(data + i);
            if (!count || count > w * h - total) { reply(p, 1, NULL, 0); return; }
            total += count;
        }
        if (total != w * h) break;
        uint32_t pixel = 0;
        for (size_t i = 8; i < len; i += 4) {
            for (uint32_t n = 0; n < u16(data + i); ++n, ++pixel) {
                size_t dest = ((y + pixel / w) * TC_WIDTH + x + pixel % w) * 2;
                if (p->packet[4] == 8) {
                    p->frame[dest] ^= data[i + 2]; p->frame[dest + 1] ^= data[i + 3];
                } else memcpy(p->frame + dest, data + i + 2, 2);
            }
        }
        reply(p, 0, NULL, 0); return;
    }
    case 3: // PRESENT: publish staged rectangles, return framebuffer CRC.
        if (len) break;
        p->present(p->frame);
        put32(result, tc_crc32(0, p->frame, TC_FRAME_BYTES));
        reply(p, 0, result, 4); return;
    case 4: { // INJECT_TAP: simulator-only input, translated to the 320x240 scene.
        if (len != 4) break;
        uint16_t x = u16(data), y = u16(data + 2);
        if (x < 80 || x >= 400 || y < 40 || y >= 280) break;
        put16(result, x - 80); put16(result + 2, y - 40);
        reply(p, 0, result, 4); return;
    }
    case 5: // CLEAR staging framebuffer; call PRESENT to display it.
        if (len) break;
        memset(p->frame, 0, TC_FRAME_BYTES);
        reply(p, 0, NULL, 0); return;
    default:
        reply(p, 3, NULL, 0); return;
    }
    reply(p, 1, NULL, 0);
}

void tc_reset(tc_protocol *p) { p->used = 0; }
void tc_init(tc_protocol *p, uint8_t *frame, tc_write_fn write, tc_present_fn present) {
    memset(p, 0, sizeof(*p));
    p->frame = frame; p->write = write; p->present = present;
    memset(frame, 0, TC_FRAME_BYTES);
}

void tc_feed(tc_protocol *p, const uint8_t *data, size_t size) {
    for (size_t i = 0; i < size; ++i) {
        uint8_t b = data[i];
        if (p->used < 4 && b != (uint8_t)"TCF1"[p->used]) {
            p->used = b == 'T' ? 1 : 0;
            p->packet[0] = 'T';
            continue;
        }
        p->packet[p->used++] = b;
        if (p->used == TC_HEADER && (p->packet[5] != 1 || u16(p->packet + 6) > TC_MAX_PAYLOAD)) {
            reply(p, 1, NULL, 0); tc_reset(p);
        } else if (p->used >= TC_HEADER && p->used == TC_HEADER + u16(p->packet + 6)) {
            dispatch(p); tc_reset(p);
        }
    }
}
