#pragma once
#include <stdbool.h>
#include <stdint.h>
#include "protocol.h"
// JC4827W543 LCD MADCTL=0xc0 reverses both axes. GT911 reports its
// native orientation. Convert physical contacts once, before gesture detection.
// USB QA samples already use displayed panel coordinates and bypass this map.
static inline bool touch_to_display(uint16_t *x, uint16_t *y) {
    if (*x >= TC_WIDTH || *y >= TC_HEIGHT) return false;
    *x = TC_WIDTH - 1 - *x;
    *y = TC_HEIGHT - 1 - *y;
    return true;
}
