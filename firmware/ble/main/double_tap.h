#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
// Board clock, not BLE arrival time. First tap remains immediate; second emits kind 3.
typedef struct { bool pending; uint16_t x,y; uint32_t at,epoch; } DoubleTap;
static inline uint8_t double_tap_release(DoubleTap *d, uint16_t *x, uint16_t *y,
                                       uint32_t now, uint32_t held, uint32_t epoch) {
    bool twice=d->pending && d->epoch==epoch && held<=350 &&
        (uint32_t)(now-d->at)<=450 && abs((int)*x-d->x)<=8 && abs((int)*y-d->y)<=8;
    if(twice) {
        // Keep the first tap's hit target, including near seat boundaries.
        *x=d->x;*y=d->y;d->pending=false;return 3;
    }
    *d=(DoubleTap){.pending=held<=350,.x=*x,.y=*y,.at=now,.epoch=epoch};
    return 1;
}
