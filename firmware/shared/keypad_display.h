#ifndef CAFE_KEYPAD_DISPLAY_H
#define CAFE_KEYPAD_DISPLAY_H
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#define KD_ROWS 4
#define KD_COLS 16
#define KD_PACKET_SIZE 20
#define KD_STALE_MS 8000
/* One 20-byte acknowledged ATT write per row works at the default MTU.
 * A frame is published only after four ordered rows with the same sequence. */
typedef struct {
    char pending[KD_ROWS][KD_COLS+1], lines[KD_ROWS][KD_COLS+1];
    uint8_t sequence, next_row;
    bool valid;
    uint64_t started, updated;
} KdFrame;
static inline bool kd_receive(KdFrame *f, const uint8_t *p, unsigned len, uint64_t now) {
    if(len!=KD_PACKET_SIZE || p[0]!=1 || p[2]>=KD_ROWS || p[19]!=0) return false;
    for(unsigned i=3;i<19;i++) if(p[i]<32 || p[i]>126) return false;
    unsigned row=p[2];
    if(row==0) { f->sequence=p[1]; f->next_row=0; f->started=now; }
    if(p[1]!=f->sequence || row!=f->next_row || now-f->started>2000) return false;
    memcpy(f->pending[row],p+3,KD_COLS); f->pending[row][KD_COLS]=0;
    if(++f->next_row==KD_ROWS) {
        memcpy(f->lines,f->pending,sizeof(f->lines)); f->valid=true; f->updated=now;
    }
    return true;
}
static inline bool kd_fresh(const KdFrame *f,uint64_t now) {
    return f->valid && now-f->updated<KD_STALE_MS;
}
#endif
