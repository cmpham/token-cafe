#pragma once
#include <stdbool.h>
#include <stdint.h>
#include "protocol.h"
// The GT911 sensor is mounted opposite the upright LCD (MADCTL=0x00).
// Physical top-left taps read near (419,181); map them to (60,90).
// Calibrate touch independently of the LCD register's axis bits.
// USB QA samples already use displayed panel coordinates and bypass this map.
static inline bool touch_to_display(uint16_t *x, uint16_t *y) {
    if (*x >= TC_WIDTH || *y >= TC_HEIGHT) return false;
    *x = TC_WIDTH - 1 - *x;
    *y = TC_HEIGHT - 1 - *y;
    return true;
}
