/* SPDX-License-Identifier: MIT */
#include <zephyr/sys/util.h>
#include <stdio.h>
#include <string.h>
#include <lvgl.h>
#include <zmk/battery.h>
#include <zmk/display/status_screen.h>
#include "transport.h"
#include "oled_icons.h"

static lv_obj_t *screen, *labels[5];
static char mode, provider;
static bool lit=true, silenced;
static uint32_t seen_input;
static char alert_identity[34];

static void icon(lv_draw_ctx_t *ctx,int x,int y,bool codex,int scale) {
    const unsigned char *pixels=codex?&codex_icon[0][0]:&claude_icon[0][0];
    int w=codex?11:7,h=codex?9:7;
    lv_draw_rect_dsc_t style; lv_draw_rect_dsc_init(&style);
    style.bg_color=lv_color_white(); style.bg_opa=LV_OPA_COVER;
    for(int row=0;row<h;row++) for(int col=0;col<w;col++) if(pixels[row*w+col]) {
        lv_area_t area={x+col*scale,y+row*scale,x+(col+1)*scale-1,y+(row+1)*scale-1};
        lv_draw_rect(ctx,&style,&area);
    }
}
static void draw(lv_event_t *event) {
    lv_draw_ctx_t *ctx=lv_event_get_draw_ctx(event);
    if(mode=='J') { icon(ctx,4,0,false,1); icon(ctx,70,0,true,1); }
    else if(mode=='I') { icon(ctx,4,9,false,2); icon(ctx,66,7,true,2); }
    else if(mode=='N') icon(ctx,2,2,provider=='2',1);
    else if(mode=='A' && lit) {
        if(provider=='1') icon(ctx,57,9,false,2);
        else if(provider=='2') icon(ctx,53,7,true,2);
        else { icon(ctx,34,9,false,2); icon(ctx,72,7,true,2); }
    }
}
static void label(unsigned i,const char *text,int x,int y,int width) {
    lv_obj_set_style_text_font(labels[i],&lv_font_unscii_8,0);
    lv_obj_set_pos(labels[i],x,y); lv_obj_set_size(labels[i],width,8);
    if(strcmp(lv_label_get_text(labels[i]),text)) lv_label_set_text(labels[i],text);
}
static void percentage(unsigned i,const char *text,int x,int y) {
    label(i,text,x,y,60);
    lv_obj_set_style_text_font(labels[i],&lv_font_montserrat_12,0);
    // Digits, %, hyphen and space fit above the font's 3px descender area.
    lv_obj_set_height(labels[i],12);
}
static void refresh(lv_timer_t *timer) {
    ARG_UNUSED(timer);
    char lines[4][17]={"CAFE CONNECTING", "START TOKEN CAFE", "ON YOUR MAC", ""};
    token_cafe_display_lines(lines);
    bool weekly=!strncmp(lines[0],"@TC3I",5);
    char next=!strncmp(lines[0],"@TC4I",5)?'J':weekly?'I':0;
    if(!strncmp(lines[0],"@TC2",4) && strchr("INA",lines[0][4]) && lines[0][4]) next=lines[0][4];
    char identity[34]; memcpy(identity,lines[0],17); memcpy(identity+17,lines[1],17);
    if(next!='A' || memcmp(identity,alert_identity,sizeof(identity))) silenced=false;
    memcpy(alert_identity,identity,sizeof(identity));
    uint32_t input=token_cafe_input_generation();
    if(input!=seen_input) { silenced=true; seen_input=input; }
    mode=next;
    provider=mode=='N' ? (!strncmp(lines[1],"CODEX",5)?'2':'1') : lines[0][5];
    // Rendering owns blink timing: no extra USB/BLE writes, stop locally on input.
    lit=silenced || (lv_tick_get()/500)%2==0;
    for(unsigned i=0;i<5;i++) label(i,"",0,8*i,128);
    if(mode=='J') {
        char cli[5]={0},desktop[5]={0},codex[5]={0};
        memcpy(cli,lines[3],4); memcpy(desktop,lines[3]+4,4); memcpy(codex,lines[3]+8,4);
        label(0,lines[1],24,0,32); label(1,lines[2],94,0,32);
        percentage(2,cli,4,8); percentage(3,desktop,4,20); percentage(4,codex,68,8);
    } else if(mode=='I') {
        label(0,lines[1],24,weekly?5:12,32); label(1,lines[2],94,weekly?5:12,32);
        if(weekly) {
            char left[5]={0},right[5]={0};
            memcpy(left,lines[3],4); memcpy(right,lines[3]+8,4);
            label(2,left,24,19,32); label(3,right,94,19,32);
        }
    } else if(mode=='N') {
        label(0,lines[1],20,0,104); label(1,lines[2],0,16,128); label(2,lines[3],0,24,128);
    } else if(mode!='A') {
        size_t n=strlen(lines[0]);while(n<12) lines[0][n++]=' ';
        snprintf(lines[0]+12,5,"%3u%%",(unsigned)zmk_battery_state_of_charge());
        for(unsigned i=0;i<4;i++) label(i,lines[i],0,8*i,128);
    }
    lv_obj_invalidate(screen);
}
lv_obj_t *zmk_display_status_screen(void) {
    screen=lv_obj_create(NULL);
    lv_obj_set_style_bg_color(screen,lv_color_black(),0);
    lv_obj_set_style_bg_opa(screen,LV_OPA_COVER,0);
    lv_obj_set_style_pad_all(screen,0,0);
    lv_obj_set_style_border_width(screen,0,0);
    lv_obj_add_event_cb(screen,draw,LV_EVENT_DRAW_MAIN,NULL);
    for(int i=0;i<5;i++) {
        labels[i]=lv_label_create(screen);
        lv_obj_set_style_text_font(labels[i],&lv_font_unscii_8,0);
        lv_obj_set_style_text_color(labels[i],lv_color_white(),0);
        lv_obj_set_style_pad_all(labels[i],0,0);
        lv_label_set_long_mode(labels[i],LV_LABEL_LONG_CLIP);
        lv_label_set_text(labels[i],"");
    }
    refresh(NULL); lv_timer_create(refresh,100,NULL);
    return screen;
}
