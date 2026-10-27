#pragma once
#include <stdbool.h>
#include <stdint.h>
#define SCREEN_IDLE_MS (10u * 60u * 1000u)
typedef struct { uint32_t last_touch; bool asleep, waking; } ScreenIdle;
// Unsigned subtraction also works across the millisecond clock rollover.
// Consume the entire waking contact, so waking cannot select/open a session.
static inline bool screen_idle_sample(ScreenIdle *s, bool pressed, uint32_t now) {
    if (pressed) {
        s->last_touch = now;
        if (s->asleep) { s->asleep = false; s->waking = true; }
    } else if (!s->asleep && (uint32_t)(now - s->last_touch) >= SCREEN_IDLE_MS) {
        s->asleep = true;
    }
    if (s->waking) {
        if (!pressed) s->waking = false;
        return false;
    }
    return !s->asleep;
}
