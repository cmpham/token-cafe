#include "protocol.h"
#include <string.h>
#ifdef ESP_PLATFORM
#include "esp_rom_crc.h"
#endif

static uint16_t u16(const uint8_t *p) { return p[0] | ((uint16_t)p[1] << 8); }
static uint32_t u32(const uint8_t *p) { return u16(p) | ((uint32_t)u16(p + 2) << 16); }
static void put16(uint8_t *p, uint16_t v) { p[0] = v; p[1] = v >> 8; }
static void put32(uint8_t *p, uint32_t v) { put16(p, v); put16(p + 2, v >> 16); }

uint32_t tc_crc32(uint32_t crc, const uint8_t *data, size_t size) {
#ifdef ESP_PLATFORM
    return esp_rom_crc32_le(crc, data, size);
#else
    crc = ~crc;
    for (size_t i = 0; i < size; ++i) {
        crc ^= data[i];
        for (int bit = 0; bit < 8; ++bit)
            crc = (crc >> 1) ^ (0xedb88320u & (0u - (crc & 1)));
    }
    return ~crc;
#endif
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

// LZ4 block decoder. Validate all lengths and offsets before mutating staging.
// Full-width rectangles are contiguous, so overlap copies need no scratch RAM.
static bool lz4(const uint8_t *src, size_t len, uint8_t *dst, size_t size) {
    for (int pass=0; pass<2; ++pass) {
        size_t i=0, out=0;
        while (i<len) {
            uint8_t token=src[i++];
            size_t literals=token>>4;
            if (literals==15) {
                uint8_t n;
                do { if(i==len) return false; n=src[i++]; literals+=n; } while(n==255);
            }
            if(literals>len-i || literals>size-out) return false;
            if(pass) memcpy(dst+out,src+i,literals);
            i+=literals;out+=literals;
            if(i==len) break;
            if(len-i<2) return false;
            size_t offset=u16(src+i);i+=2;
            if(!offset || offset>out) return false;
            size_t count=(token&15)+4;
            if((token&15)==15) {
                uint8_t n;
                do { if(i==len) return false; n=src[i++];count+=n; } while(n==255);
            }
            if(count>size-out) return false;
            if(pass) for(size_t j=0;j<count;j++) dst[out+j]=dst[out+j-offset];
            out+=count;
        }
        if(out!=size) return false;
    }
    return true;
}

static void dispatch(tc_protocol *p) {
    const uint8_t *data = p->packet + TC_HEADER;
    size_t len = u16(p->packet + 6);
    uint8_t result[16] = {0};
    if (tc_crc32(tc_crc32(0, p->packet, 12), data, len) != u32(p->packet + 12)) {
        reply(p, 2, NULL, 0); return;
    }
    switch (p->packet[4]) {
    case 1: // HELLO: dimensions and maximum payload, RGB565 little endian.
        if (len) break;
        put16(result, TC_WIDTH); put16(result + 2, TC_HEIGHT);
        put16(result + 4, TC_MAX_PAYLOAD); result[6] = 1;
        reply(p, 0, result, 7); return;
    case 16:
    case 2: { // RECT: validate the entire update before changing memory.
        if (len < 8) break;
        uint32_t x = u16(data), y = u16(data + 2);
        uint32_t w = u16(data + 4), h = u16(data + 6);
        if (!w || !h || x + w > TC_WIDTH || y + h > TC_HEIGHT ||
            len != 8 + w * h * 2) break;
        for (uint32_t row = 0; row < h; ++row)
            memcpy(p->frame + ((y + row) * TC_WIDTH + x) * 2, data + 8 + row * w * 2, w * 2);
        reply(p, 0, result, p->packet[4]==16 && p->input && p->input(result) ? (p->input_size ? p->input_size : 5) : 0); return;
    }
    case 14:
    case 15:
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
                if (p->packet[4] == 8 || p->packet[4] == 15) {
                    p->frame[dest] ^= data[i + 2]; p->frame[dest + 1] ^= data[i + 3];
                } else memcpy(p->frame + dest, data + i + 2, 2);
            }
        }
        reply(p, 0, result, (p->packet[4]==14 || p->packet[4]==15) && p->input && p->input(result) ? (p->input_size ? p->input_size : 5) : 0); return;
    }
    case 10:
    case 13: { // Full-width LZ4 rectangle (13 = XOR); reply piggybacks one touch event.
        if(len<9 || u16(data)!=0 || u16(data+4)!=TC_WIDTH) break;
        uint32_t y=u16(data+2), h=u16(data+6);
        if(!h || y+h>TC_HEIGHT) break;
        uint8_t *dest=p->frame+y*TC_WIDTH*2;
        size_t size=h*TC_WIDTH*2;
        if(p->packet[4]==13) {
            if(!p->scratch || !lz4(data+8,len-8,p->scratch,size)) break;
            for(size_t i=0;i<size;i++) dest[i]^=p->scratch[i];
        } else if(!lz4(data+8,len-8,dest,size)) break;
        reply(p,0,result,p->input && p->input(result) ? (p->input_size ? p->input_size : 5) : 0);return;
    }
    case 11: // Abort staged update, preserving the currently displayed image.
        if(len || !p->restore) break;
        p->restore(p->frame);reply(p,0,NULL,0);return;
    case 12: // Fast-path capability, explicitly negotiated before use.
        if(len) break;
        result[0]=p->scratch ? 3 : 1;result[1]=p->restore ? 1 : 0;
        reply(p,0,result,2);return;
    case 3: // PRESENT: publish staged rectangles, return framebuffer CRC.
        if (len) break;
        p->present(p->frame);
        put32(result, tc_crc32(0, p->frame, TC_FRAME_BYTES));
        reply(p, 0, result, 4); return;
    case 4: { // INJECT_TAP: simulator-only input, translated to the 480x272 scene.
        if (len != 4) break;
        uint16_t x = u16(data), y = u16(data + 2);
        if (x >= TC_WIDTH || y >= TC_HEIGHT) break;
        put16(result, x * TC_SCENE_WIDTH / TC_WIDTH);
        put16(result + 2, y * TC_SCENE_HEIGHT / TC_HEIGHT);
        reply(p, 0, result, 4); return;
    }
    case 9: // INPUT: optional queued tap/scroll, kind + panel x/y little endian.
        if (len) break;
        reply(p, 0, result, p->input && p->input(result) ? (p->input_size ? p->input_size : 5) : 0); return;
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
