#include <assert.h>
#include <stdio.h>
#include "ble/main/touch_gesture.h"
#include "ble/main/touch_coordinates.h"
static uint16_t ox,oy;
static uint8_t sample(TouchGesture *g,int count,int x,int y,uint32_t at,uint32_t epoch) {
    return touch_sample(g,count,x,y,at,epoch,&ox,&oy);
}
int main(void) {
    // The taller footer begins at y=212; the pixel above remains a patron tap.
    TouchGesture edge={0};
    assert(sample(&edge,1,470,211,10,1)==0);
    assert(sample(&edge,0,470,211,50,1)==1);
    TouchGesture rail={0};
    assert(sample(&rail,1,470,212,10,1)==2);

    // Real-controller orientation, including every seat and panel endpoints.
    for (unsigned row=0;row<2;row++) for (unsigned col=0;col<4;col++) {
        uint16_t px=col*120+60,py=row*82+91;
        uint16_t x=479-px,y=271-py;
        assert(touch_to_display(&x,&y) && x==px && y==py);
        TouchGesture physical={0};
        assert(sample(&physical,1,x,y,10,1)==0);
        assert(sample(&physical,0,0,0,50,1)==1 && ox==px && oy==py);
        assert(sample(&physical,1,x,y,100,1)==0);
        assert(sample(&physical,0,0,0,150,1)==3 && ox==px && oy==py);
    }
    uint16_t x=0,y=0;
    assert(touch_to_display(&x,&y) && x==479 && y==271);
    assert(touch_to_display(&x,&y) && x==0 && y==0);
    x=480;assert(!touch_to_display(&x,&y));
    x=0;y=272;assert(!touch_to_display(&x,&y));
    TouchGesture g={0};
    assert(sample(&g,1,60,91,100,1)==0);
    touch_cancel(&g); // I2C read fails while finger remains down.
    assert(sample(&g,1,60,91,150,1)==0);
    assert(sample(&g,0,60,91,200,1)==0); // must NOT become a fresh tap
    assert(sample(&g,1,60,91,250,1)==0);
    assert(sample(&g,0,60,91,300,1)==1); // next complete contact works
    assert(sample(&g,1,62,93,350,1)==0);
    assert(sample(&g,0,62,93,400,1)==3 && ox==60 && oy==91);
    assert(sample(&g,1,60,91,450,1)==0);
    assert(sample(&g,2,60,91,460,1)==0);
    assert(sample(&g,1,60,91,470,1)==0);
    assert(sample(&g,0,60,91,480,1)==0);
    assert(sample(&g,1,470,230,500,1)==2);
    assert(sample(&g,1,470,250,520,2)==0); // reconnect invalidates held scroll
    assert(sample(&g,0,470,250,540,2)==0);
    assert(sample(&g,1,60,91,600,2)==0);
    assert(sample(&g,0,60,91,650,3)==0); // held tap across reconnect
    assert(sample(&g,1,60,91,700,3)==0);
    assert(sample(&g,1,80,91,710,3)==0);
    assert(sample(&g,0,80,91,750,3)==0); // drag cannot tap
    puts("PASS shared gesture recognizer: read failure, multi-touch, drag, reconnect, double tap");
}
