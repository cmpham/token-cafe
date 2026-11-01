#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

typedef struct { int left, top, right, bottom; } FrameDamage;

// Find the smallest changed rectangle in a strip. Skipping unchanged outer rows
// saves LCD traffic as well as staging work for short animations and feedback.
static inline bool frame_damage_find(const uint16_t *previous,const uint16_t *frame,
                                     int stride,int y,int rows,FrameDamage *damage) {
    int top=y,bottom=y+rows-1;
    while(top<=bottom&&!memcmp(previous+top*stride,frame+top*stride,(size_t)stride*sizeof(*frame)))top++;
    if(top>bottom)return false;
    while(bottom>top&&!memcmp(previous+bottom*stride,frame+bottom*stride,(size_t)stride*sizeof(*frame)))bottom--;
    int left=stride,right=-1;
    for(int row=top;row<=bottom;row++) {
        const uint16_t *before=previous+row*stride,*after=frame+row*stride;
        for(int col=0;col<left;col++)if(before[col]!=after[col]){left=col;break;}
        for(int col=stride-1;col>right;col--)if(before[col]!=after[col]){right=col;break;}
        if(left==0&&right==stride-1)break;
    }
    *damage=(FrameDamage){left,top,right,bottom};
    return true;
}

// Source/previous pixels are native RGB565; the panel takes the high byte first.
// Retain exactly the pixels staged for transmission, including unchanged pixels
// inside the rectangle. No full-frame copy is needed after presenting strips.
static inline void frame_damage_stage(uint16_t *dma,uint16_t *previous,const uint16_t *frame,
                                      int stride,FrameDamage damage) {
    int width=damage.right-damage.left+1;
    for(int row=damage.top;row<=damage.bottom;row++) {
        const uint16_t *source=frame+row*stride+damage.left;
        for(int col=0;col<width;col++)dma[col]=(uint16_t)((source[col]<<8)|(source[col]>>8));
        if(previous)memcpy(previous+row*stride+damage.left,source,(size_t)width*sizeof(*source));
        dma+=width;
    }
}
