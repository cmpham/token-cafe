#include <assert.h>
#include <stdio.h>
#include "ble/main/screen_idle.h"
int main(void) {
    ScreenIdle s={.last_touch=100};
    assert(screen_idle_sample(&s,false,600099) && !s.asleep);
    assert(!screen_idle_sample(&s,false,600100) && s.asleep);
    assert(!screen_idle_sample(&s,false,700000) && s.asleep);
    assert(!screen_idle_sample(&s,true,700001) && !s.asleep); // wake only
    assert(!screen_idle_sample(&s,true,700050)); // held waking contact
    assert(!screen_idle_sample(&s,false,700100)); // release consumed
    assert(screen_idle_sample(&s,true,700200)); // next tap works
    assert(screen_idle_sample(&s,false,700250));
    assert(screen_idle_sample(&s,false,1300199));
    assert(!screen_idle_sample(&s,false,1300200) && s.asleep);
    s=(ScreenIdle){.last_touch=UINT32_MAX-100};
    assert(screen_idle_sample(&s,false,599898));
    assert(!screen_idle_sample(&s,false,599899) && s.asleep);
    s=(ScreenIdle){0};
    for(uint32_t t=0;t<=1200000;t+=20) assert(screen_idle_sample(&s,true,t));
    assert(!s.asleep); // Holding a finger is activity, not inactivity.
    puts("PASS idle threshold, wake consumption, activity reset, held contact, clock rollover");
}
