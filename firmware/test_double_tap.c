#include <assert.h>
#include <stdio.h>
#include "ble/main/double_tap.h"
static uint8_t tap(DoubleTap *d,uint16_t x,uint16_t y,uint32_t at,uint32_t held,uint32_t epoch) {
    return double_tap_release(d,&x,&y,at,held,epoch);
}
int main(void) {
    DoubleTap d={0};
    assert(tap(&d,60,91,100,50,1)==1);
    uint16_t x=67,y=98;
    assert(double_tap_release(&d,&x,&y,550,50,1)==3 && x==60 && y==91);
    assert(tap(&d,60,91,600,50,1)==1); // triple tap does not activate twice
    assert(tap(&d,60,91,1051,50,1)==1); // timeout
    assert(tap(&d,69,91,1100,50,1)==1); // outside tolerance
    assert(tap(&d,69,91,1150,351,1)==1); // long second hold
    assert(tap(&d,69,91,1200,50,1)==1); // long first hold cannot start a pair
    assert(tap(&d,69,91,1250,50,2)==1); // reconnect invalidates pair
    d.pending=false; // drag / scroll / multi-touch / reset
    assert(tap(&d,69,91,1300,50,2)==1);
    d=(DoubleTap){0};
    assert(tap(&d,60,91,UINT32_MAX-99,50,3)==1);
    assert(tap(&d,60,91,100,50,3)==3); // board clock rollover
    puts("PASS double-tap timing, radius, anchor, triple, holds, epoch, cancel, rollover");
}
