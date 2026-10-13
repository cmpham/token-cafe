#pragma once
#include <string.h>
#include "protocol.h"
#include "double_tap.h"
typedef struct { bool down,cancel,scroll; uint16_t start_x,start_y,last_y; uint32_t epoch,started; DoubleTap taps; } TouchGesture;
static inline uint8_t touch_sample(TouchGesture *g,uint8_t count,uint16_t x,uint16_t y,
                                   uint32_t now,uint32_t epoch,uint16_t *out_x,uint16_t *out_y) {
    uint8_t event=0;*out_x=x;*out_y=y;
    if(g->down && g->epoch!=epoch) {g->cancel=true;g->taps.pending=false;}
    if(count) {
        if(count>1) g->cancel=true;
        if(!g->down) {
            g->epoch=epoch;g->started=now;
            g->down=true;g->start_x=x;g->start_y=y;g->last_y=y;
            g->cancel=count!=1 || x>=TC_WIDTH || y>=TC_HEIGHT;
            g->scroll=!g->cancel && x*TC_SCENE_WIDTH/TC_WIDTH>=276 && y*TC_SCENE_HEIGHT/TC_HEIGHT>=192;
            if(g->scroll) {g->taps.pending=false;event=2;}
        } else if(!g->cancel) {
            if(x>=TC_WIDTH || y>=TC_HEIGHT) g->cancel=true;
            else if(g->scroll) {
                if(abs((int)y-g->last_y)>=3) { event=2;g->last_y=y; }
            } else if(abs((int)x-g->start_x)>8 || abs((int)y-g->start_y)>8) g->cancel=true;
        }
    } else if(g->down) {
        if(!g->cancel && !g->scroll) {
            uint16_t x=g->start_x,y=g->start_y;
            uint8_t kind=double_tap_release(&g->taps,&x,&y,now,now-g->started,g->epoch);
            if(g->epoch==epoch) {event=kind;*out_x=x;*out_y=y;}
        } else g->taps.pending=false;
        DoubleTap taps=g->taps;
        memset(g,0,sizeof(*g));g->taps=taps;
    }
    return event;
}
// A read error does not prove release. Ignore the remaining contact until an
// explicit no-contact sample arrives; otherwise a held finger can become a tap.
static inline void touch_cancel(TouchGesture *g) {
    memset(g,0,sizeof(*g));g->down=true;g->cancel=true;
}
