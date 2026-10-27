#pragma once
#include <stdbool.h>
#include <stdint.h>
// Debounced active-low switch. Startup must first see a stable release.
typedef struct { bool raw, stable, ready; uint64_t changed; } ExternalButton;
static inline ExternalButton external_button_init(bool pressed, uint64_t now) {
    return (ExternalButton){.raw=pressed,.stable=pressed,.ready=false,.changed=now};
}
static inline bool external_button_sample(ExternalButton *b,bool pressed,uint64_t now) {
    if(pressed!=b->raw){b->raw=pressed;b->changed=now;}
    if(now-b->changed<30)return false;
    bool edge=b->stable!=b->raw;b->stable=b->raw;
    if(!b->ready){if(!b->stable)b->ready=true;return false;}
    return edge;
}
