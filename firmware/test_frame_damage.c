// Native LCD simulation: every transmitted byte must reconstruct the requested
// framebuffer, including strip edges and feedback overwritten by a later frame.
#include "ble/main/frame_damage.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

enum { WIDTH=480, HEIGHT=272, STRIP=32, PIXELS=WIDTH*HEIGHT };
static uint16_t previous[PIXELS],frame[PIXELS],panel[PIXELS],dma[WIDTH*STRIP];
static bool simulate=true;
static void observe_dma(void) {
    // Model the SPI driver's consumption; retain every staged store in profiles.
    __asm__ __volatile__("" : : "r"(dma) : "memory");
}
static uint32_t random_state=17;
static uint32_t random_value(void) {
    random_state=random_state*1664525u+1013904223u;
    return random_state;
}
static bool reference_damage(const uint16_t *before,const uint16_t *after,
                             int width,int y,int rows,FrameDamage *damage) {
    *damage=(FrameDamage){width,y+rows,-1,-1};
    for(int row=y;row<y+rows;row++)for(int col=0;col<width;col++) {
        if(before[row*width+col]==after[row*width+col])continue;
        if(col<damage->left)damage->left=col;
        if(col>damage->right)damage->right=col;
        if(row<damage->top)damage->top=row;
        if(row>damage->bottom)damage->bottom=row;
    }
    return damage->right>=damage->left;
}
static void exact_bounds(void) {
    uint16_t before[35]={0},after[35]={0};
    for(int iteration=0;iteration<10000;iteration++) {
        for(int i=0;i<35;i++)after[i]=(random_value()%7==0)?(uint16_t)random_value():before[i];
        for(int y=0;y<5;y++)for(int rows=1;rows<=5-y;rows++) {
            FrameDamage expected,actual;
            bool changed=reference_damage(before,after,7,y,rows,&expected);
            assert(frame_damage_find(before,after,7,y,rows,&actual)==changed);
            if(changed) {
                assert(actual.left==expected.left&&actual.right==expected.right);
                assert(actual.top==expected.top&&actual.bottom==expected.bottom);
            }
        }
        memcpy(before,after,sizeof(before));
    }
}
static size_t stage(FrameDamage damage) {
    int width=damage.right-damage.left+1,rows=damage.bottom-damage.top+1;
    assert(width>0&&width<=WIDTH&&rows>0&&rows<=STRIP);
    frame_damage_stage(dma,previous,frame,WIDTH,damage);
    observe_dma();
    if(!simulate)return (size_t)width*rows*2;
    const uint8_t *bytes=(const uint8_t *)dma;
    for(int row=0;row<rows;row++)for(int col=0;col<width;col++) {
        int offset=(row*width+col)*2;
        // Decode the high-byte-first LCD wire format, independently of staging.
        panel[(damage.top+row)*WIDTH+damage.left+col]=(uint16_t)(bytes[offset]*256u+bytes[offset+1]);
    }
    return (size_t)width*rows*2;
}
static size_t present(void) {
    size_t bytes=0;
    for(int y=0;y<HEIGHT;y+=STRIP) {
        int rows=HEIGHT-y<STRIP?HEIGHT-y:STRIP;
        FrameDamage damage;
        if(frame_damage_find(previous,frame,WIDTH,y,rows,&damage))bytes+=stage(damage);
    }
    if(simulate) {
        assert(!memcmp(previous,frame,sizeof(frame)));
        assert(!memcmp(panel,frame,sizeof(frame)));
    }
    return bytes;
}
static void transitions(void) {
    assert(present()==0);
    const int ys[]={0,31,32,63,64,255,256,271};
    const int xs[]={0,1,239,479};
    for(size_t y=0;y<sizeof(ys)/sizeof(*ys);y++)for(size_t x=0;x<sizeof(xs)/sizeof(*xs);x++) {
        frame[ys[y]*WIDTH+xs[x]]^=0xabcd;
        assert(present()==2);
        assert(present()==0);
    }
    // A full repaint, every RGB565 value, and the final partial strip.
    for(int i=0;i<PIXELS;i++)frame[i]=(uint16_t)i;
    present();
    assert(present()==0);
    for(int iteration=0;iteration<300;iteration++) {
        unsigned count=iteration%2?1:2000;
        for(unsigned i=0;i<count;i++)frame[random_value()%PIXELS]=(uint16_t)random_value();
        present();
    }
    // Reproduce the immediate pressed-feedback stage, then a fresh engine frame
    // that must restore all feedback pixels without retaining a stale rectangle.
    for(int edge=0;edge<4;edge++) {
        int left=edge&1?WIDTH-11:0,top=edge&2?HEIGHT-11:0;
        FrameDamage feedback={left,top,left+10,top+10};
        uint16_t saved[121];
        for(int row=0;row<11;row++)for(int col=0;col<11;col++) {
            int p=(top+row)*WIDTH+left+col;
            saved[row*11+col]=frame[p];frame[p]=0xfecb;
        }
        assert(stage(feedback)==242);
        assert(present()==0);
        for(int row=0;row<11;row++)for(int col=0;col<11;col++)frame[(top+row)*WIDTH+left+col]=saved[row*11+col];
        present();
    }
    // Legacy callers stage a complete strip without keeping a previous frame.
    frame_damage_stage(dma,NULL,frame,WIDTH,(FrameDamage){0,256,WIDTH-1,HEIGHT-1});
    const uint8_t *bytes=(const uint8_t *)dma;
    for(int i=0;i<WIDTH*16;i++)assert((uint16_t)(bytes[i*2]*256u+bytes[i*2+1])==frame[WIDTH*256+i]);
}
// Preserve the original algorithm for a CPU/traffic comparison on the same
// synthetic frames. This is profiling evidence, not physical SPI/PSRAM timing.
static size_t baseline_present(void) {
    size_t bytes=0;
    for(int y=0;y<HEIGHT;y+=STRIP) {
        int rows=HEIGHT-y<STRIP?HEIGHT-y:STRIP;
        if(!memcmp(previous+y*WIDTH,frame+y*WIDTH,(size_t)rows*WIDTH*2))continue;
        int left=WIDTH,right=-1;
        for(int row=y;row<y+rows;row++)for(int col=0;col<WIDTH;col++) {
            if(previous[row*WIDTH+col]!=frame[row*WIDTH+col]) {
                if(col<left)left=col;
                if(col>right)right=col;
            }
        }
        if(right<left)continue;
        int width=right-left+1;
        uint8_t *dest=(uint8_t *)dma;
        const uint8_t *source=(const uint8_t *)frame;
        for(int row=0;row<rows;row++)for(int col=0;col<width;col++) {
            int dst=(row*width+col)*2,src=((y+row)*WIDTH+left+col)*2;
            dest[dst]=source[src+1];dest[dst+1]=source[src];
        }
        observe_dma();
        bytes+=(size_t)width*rows*2;
    }
    memcpy(previous,frame,sizeof(frame));
    return bytes;
}
static double now_seconds(void) {
    struct timespec t;
    assert(timespec_get(&t,TIME_UTC)==TIME_UTC);
    return (double)t.tv_sec+(double)t.tv_nsec/1e9;
}
static void benchmark(void) {
    simulate=false;
    for(int workload=0;workload<4;workload++)for(int baseline=1;baseline>=0;baseline--) {
        memset(previous,0,sizeof(previous));memset(frame,0,sizeof(frame));memset(panel,0,sizeof(panel));
        size_t bytes=0;
        double started=now_seconds();
        for(int iteration=0;iteration<1000;iteration++) {
            if(workload==1)frame[100*WIDTH+240]^=0xabcd;
            if(workload==2)for(int row=100;row<108;row++)for(int col=200;col<216;col++)frame[row*WIDTH+col]^=0xabcd;
            if(workload==3)for(int i=0;i<PIXELS;i++)frame[i]^=0xabcd;
            bytes+=baseline?baseline_present():present();
        }
        printf("PROFILE %s %s: %.1f us/frame, %zu LCD bytes/frame\n",
            workload==0?"unchanged":workload==1?"one-pixel":workload==2?"16x8-animation":"full-repaint",
            baseline?"baseline":"optimized",(now_seconds()-started)*1000,bytes/1000);
    }
}
int main(int argc,char **argv) {
    (void)argv;
    exact_bounds();transitions();
    puts("PASS exact damage bounds, RGB565 wire bytes, random LCD reconstruction, strip edges, feedback restore, legacy staging");
    if(argc>1)benchmark();
}
