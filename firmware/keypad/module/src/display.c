/* SPDX-License-Identifier: MIT */
#include <zephyr/sys/util.h>
#include <stdio.h>
#include <string.h>
#include <lvgl.h>
#include <zmk/battery.h>
#include <zmk/display/status_screen.h>
#include "transport.h"

static lv_obj_t *labels[4];
static void refresh(lv_timer_t *timer) {
    ARG_UNUSED(timer);
    char lines[4][17]={"CAFE CONNECTING", "START TOKEN CAFE", "ON YOUR MAC", ""};
    token_cafe_display_lines(lines);
    /* Reserve the final four columns of the first row for local battery. */
    size_t n=strlen(lines[0]);while(n<12) lines[0][n++]=' ';
    snprintf(lines[0]+12,5,"%3u%%",(unsigned)zmk_battery_state_of_charge());
    for(int i=0;i<4;i++) if(strcmp(lv_label_get_text(labels[i]),lines[i])) lv_label_set_text(labels[i],lines[i]);
}
lv_obj_t *zmk_display_status_screen(void) {
    lv_obj_t *screen=lv_obj_create(NULL);
    lv_obj_set_style_bg_color(screen,lv_color_black(),0);
    lv_obj_set_style_bg_opa(screen,LV_OPA_COVER,0);
    lv_obj_set_style_pad_all(screen,0,0);
    lv_obj_set_style_border_width(screen,0,0);
    for(int i=0;i<4;i++) {
        labels[i]=lv_label_create(screen);
        lv_obj_set_style_text_font(labels[i],&lv_font_unscii_8,0);
        lv_obj_set_style_text_color(labels[i],lv_color_white(),0);
        lv_obj_set_style_pad_all(labels[i],0,0);
        lv_label_set_long_mode(labels[i],LV_LABEL_LONG_CLIP);
        lv_obj_set_size(labels[i],128,8);lv_obj_set_pos(labels[i],0,8*i);
        lv_label_set_text(labels[i],"");
    }
    refresh(NULL); lv_timer_create(refresh,250,NULL);
    return screen;
}
