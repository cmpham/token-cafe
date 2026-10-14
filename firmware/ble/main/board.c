#include "esp_timer.h"
#include "sdkconfig.h"
#ifndef TC_SEMANTIC
#define TC_SEMANTIC 0
#endif
#include "board.h"
#include "protocol.h"
#if CONFIG_TC_BOARD_JC4827W543
#include "check.h"
#include <string.h>
#include <stdlib.h>
#include <stdatomic.h>
#include "driver/gpio.h"
#include "driver/spi_master.h"
#include "driver/i2c_master.h"
#include "esp_lcd_touch_gt911.h"
#include "esp_lcd_panel_io.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/queue.h"
#include "nv3041a_init.h"
#include "touch_gesture.h"
#include "mbedtls/sha256.h"
#include "touch_coordinates.h"
#include "screen_idle.h"
static spi_device_handle_t lcd;
static esp_lcd_touch_handle_t touch;
static QueueHandle_t events;
#define LCD_ROWS 32
static uint8_t *dma;
static void command(uint8_t cmd, const uint8_t *data, size_t len) {
    spi_transaction_t t = {.cmd=0x02, .addr=(uint32_t)cmd<<8,
        .length=len*8, .tx_buffer=data};
    ESP_ERROR_CHECK(spi_device_polling_transmit(lcd, &t));
}
// The QSPI adapter sends the four window bytes in a single register write.
static void window(uint8_t cmd, uint16_t first, uint16_t last) {
    uint8_t bytes[] = {first>>8, first&255, last>>8, last&255};
    command(cmd,bytes,sizeof(bytes));
}
static uint64_t displayed_crc;
static portMUX_TYPE fingerprint_lock=portMUX_INITIALIZER_UNLOCKED;
static uint64_t fingerprint_get(void) {
    taskENTER_CRITICAL(&fingerprint_lock); uint64_t value=displayed_crc; taskEXIT_CRITICAL(&fingerprint_lock); return value;
}
static void fingerprint_set(uint64_t value) {
    taskENTER_CRITICAL(&fingerprint_lock); displayed_crc=value; taskEXIT_CRITICAL(&fingerprint_lock);
}
// Orientation and inversion as set by the init table; re-sent because a lost
// value shows the panel default (180 degrees rotated, colours inverted).
static void panel_orientation(void){
    static const uint8_t page_on=0xa5,madctl=0xc0,page_off=0x00;
    command(0xff,&page_on,1);command(0x36,&madctl,1);command(0xff,&page_off,1);command(0x20,NULL,0);
}
#if TC_SEMANTIC
static uint8_t *previous;
static uint64_t orientation_ms;
void board_backlight(bool on){gpio_set_level(1,on);if(on)panel_orientation();}
void board_present_feedback(const uint8_t *frame,int x,int y){
    if(!previous)return;
    int left=x-10<0?0:x-10,right=x+10>=TC_WIDTH?TC_WIDTH-1:x+10;
    int top=y-10<0?0:y-10,bottom=y+10>=TC_HEIGHT?TC_HEIGHT-1:y+10;
    int width=right-left+1,rows=bottom-top+1;
    window(0x2a,left,right);window(0x2b,top,bottom);command(0x2c,NULL,0);
    for(int row=0;row<rows;row++)for(int col=0;col<width;col++){
        int dst=(row*width+col)*2,src=((top+row)*TC_WIDTH+left+col)*2;
        dma[dst]=frame[src+1];dma[dst+1]=frame[src];
        previous[src]=frame[src];previous[src+1]=frame[src+1];
    }
    spi_transaction_t t={.cmd=0x32,.addr=0x003c00,.flags=SPI_TRANS_MODE_QIO,.tx_buffer=dma,.length=width*rows*16};
    ESP_ERROR_CHECK(spi_device_polling_transmit(lcd,&t));
}
#endif
void board_present(const uint8_t *frame) {
#if TC_SEMANTIC
    uint64_t now_ms=esp_timer_get_time()/1000;
    if(now_ms-orientation_ms>=1000){orientation_ms=now_ms;panel_orientation();}
    if(!previous){previous=heap_caps_malloc(TC_FRAME_BYTES,MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);TC_CHECK(previous);memset(previous,0xff,TC_FRAME_BYTES);}
#else
    uint8_t hash[32]; TC_CHECK(mbedtls_sha256(frame,TC_FRAME_BYTES,hash,0)==0);
    uint64_t next_crc=0; for(int i=0;i<8;i++) next_crc|=(uint64_t)hash[i]<<(8*i);
    if(fingerprint_get()!=next_crc) fingerprint_set(0);
#endif
    for (int y=0;y<TC_HEIGHT;y+=LCD_ROWS) {
        int rows=TC_HEIGHT-y < LCD_ROWS ? TC_HEIGHT-y : LCD_ROWS;
        int left=0,right=TC_WIDTH-1;
#if TC_SEMANTIC
        if(!memcmp(previous+y*TC_WIDTH*2,frame+y*TC_WIDTH*2,rows*TC_WIDTH*2))continue;
        // Bound changed columns within this strip. Tiny local animation or
        // pressed-feedback updates should not transfer 480 pixels per row.
        left=TC_WIDTH;right=-1;
        const uint16_t *before=(const uint16_t *)previous,*after=(const uint16_t *)frame;
        for(int row=y;row<y+rows;row++)for(int col=0;col<TC_WIDTH;col++){
            int p=row*TC_WIDTH+col;if(before[p]!=after[p]){if(col<left)left=col;if(col>right)right=col;}
        }
        if(right<left)continue;
#endif
        int width=right-left+1;
        window(0x2a,left,right);
        window(0x2b,y,y+rows-1);
        command(0x2c,NULL,0);
        for(int row=0;row<rows;row++)for(int col=0;col<width;col++){
            int dst=(row*width+col)*2,src=((y+row)*TC_WIDTH+left+col)*2;
            dma[dst]=frame[src+1];dma[dst+1]=frame[src];
        }
        spi_transaction_t t={.cmd=0x32,.addr=0x003c00,.flags=SPI_TRANS_MODE_QIO,
            .tx_buffer=dma,.length=width*rows*16};
        ESP_ERROR_CHECK(spi_device_polling_transmit(lcd,&t));
    }
#if TC_SEMANTIC
    memcpy(previous,frame,TC_FRAME_BYTES);
#else
    fingerprint_set(next_crc);
#endif
}
typedef struct { uint32_t epoch; uint8_t data[13]; } TouchEvent;
static atomic_uint input_epoch;
static void emit(uint8_t kind,uint16_t x,uint16_t y,uint32_t epoch,uint64_t crc) {
    if(epoch!=atomic_load(&input_epoch)) return;
    TouchEvent event={.epoch=epoch,.data={kind,x&255,x>>8,y&255,y>>8}};
    for(int i=0;i<8;i++) event.data[5+i]=crc>>(8*i);
    if(xQueueSend(events,&event,0)!=pdTRUE) ESP_LOGW("TOUCH","input queue full");
    ESP_LOGI("TOUCH","%s %u,%u",kind==1?"TAP":kind==3?"DOUBLE_TAP":"SCROLL",x,y);
}
static ScreenIdle screen;
static bool touch_active;
static bool screen_activity(bool pressed) {
    bool was_asleep=screen.asleep;
    bool accept=screen_idle_sample(&screen,pressed,esp_log_timestamp());
    if (screen.asleep!=was_asleep) {
        ESP_ERROR_CHECK(gpio_set_level(1,!screen.asleep));
        ESP_LOGI("LCD","BACKLIGHT %s",screen.asleep?"OFF":"ON");
    }
    return accept;
}
static void sample(TouchGesture *g,uint8_t count,uint16_t x,uint16_t y) {
#if TC_SEMANTIC
    static uint8_t last_count;static uint16_t last_x,last_y;
    if(count==0){x=last_x;y=last_y;}
    if(count!=last_count||(count&&(x!=last_x||y!=last_y))){
        TouchEvent event={.epoch=atomic_load(&input_epoch),.data={count>1?2:count,x&255,x>>8,y&255,y>>8}};
        uint32_t at=(uint32_t)(esp_timer_get_time()/1000);memcpy(event.data+5,&at,4);
        if(xQueueSend(events,&event,0)!=pdTRUE){xQueueReset(events);event.data[0]=2;xQueueSend(events,&event,0);}
    }
    last_count=count;last_x=x;last_y=y;return;
#endif
    touch_active=count>0;
    if (!screen_activity(touch_active)) {
        if (count) touch_cancel(g); else memset(g,0,sizeof(*g));
        return;
    }
    static uint64_t touch_crc;
    uint64_t crc=fingerprint_get();
    if(count && !g->down) touch_crc=crc;
    if(touch_crc!=crc) touch_crc=0;
    if(count && !g->down) ESP_LOGI("TOUCH","DOWN %u,%u",x,y);
    uint16_t out_x,out_y;
    uint32_t epoch=atomic_load(&input_epoch);
    uint8_t kind=touch_sample(g,count,x,y,esp_log_timestamp(),epoch,&out_x,&out_y);
    if(kind) emit(kind,out_x,out_y,epoch,touch_crc);
}
#if CONFIG_TC_QA_USB
#include <stdio.h>
typedef struct { unsigned id,count,x,y; } TestSample;
static QueueHandle_t samples;
bool board_test_sample(unsigned id,unsigned count,unsigned x,unsigned y) {
    TestSample s={id,count,x,y};
    return samples && xQueueSend(samples,&s,0)==pdTRUE;
}
#endif
static void touch_task(void *arg) {
    (void)arg;
    TouchGesture gesture={0};
#if CONFIG_TC_QA_USB
    bool test_mode=false;
#endif
    for(;;) {
#if CONFIG_TC_QA_USB
        TestSample s;
        if(xQueueReceive(samples,&s,0)==pdTRUE) {
            if(s.count==253) {
                screen.last_touch=esp_log_timestamp()-s.x;
                screen_activity(touch_active);
                printf("@QA IDLE %u %u\n",s.id,!screen.asleep);
            } else if(s.count>=254) {
                test_mode=s.count==255;touch_active=false;memset(&gesture,0,sizeof(gesture));board_clear_input();
            } else if(test_mode) sample(&gesture,s.count,s.x,s.y);
            printf("@QA INPUT %u %lu %u\n",s.id,(unsigned long)(TC_SEMANTIC ? esp_timer_get_time()/1000 : esp_log_timestamp()),test_mode);
        }
        if(!test_mode)
#endif
        {
            if(esp_lcd_touch_read_data(touch)==ESP_OK) {
                uint16_t x[2]={0},y[2]={0},strength[2];uint8_t count=0;
                bool pressed=esp_lcd_touch_get_coordinates(touch,x,y,strength,&count,2);
                if (pressed && !touch_to_display(&x[0],&y[0])) sample(&gesture,2,0,0);
                else sample(&gesture,pressed ? count : 0,x[0],y[0]);
            } else sample(&gesture,2,0,0);
        }
        #if !TC_SEMANTIC
        screen_activity(touch_active);
        #endif
        vTaskDelay(pdMS_TO_TICKS(20));
    }
}
bool board_input(uint8_t out[13]) {
    TouchEvent event;
    while(xQueueReceive(events,&event,0)==pdTRUE) {
        if(event.epoch==atomic_load(&input_epoch)) {memcpy(out,event.data,13);return true;}
    }
    return false;
}
void board_clear_input(void) {
    atomic_fetch_add(&input_epoch,1); // Invalidates queued AND unfinished gestures across reconnect/auth.
    if(events) xQueueReset(events);
}
void board_init(void) {
    events=xQueueCreate(24,sizeof(TouchEvent)); TC_CHECK(events);
#if CONFIG_TC_QA_USB
    samples=xQueueCreate(32,sizeof(TestSample));TC_CHECK(samples);
#endif
    gpio_config_t bl={.pin_bit_mask=1ULL<<1,.mode=GPIO_MODE_OUTPUT};
    ESP_ERROR_CHECK(gpio_config(&bl)); gpio_set_level(1,0);
    spi_bus_config_t bus={.sclk_io_num=47,.data0_io_num=21,.data1_io_num=48,
        .data2_io_num=40,.data3_io_num=39,
        .data4_io_num=-1,.data5_io_num=-1,.data6_io_num=-1,.data7_io_num=-1,.max_transfer_sz=TC_WIDTH*LCD_ROWS*2};
    ESP_ERROR_CHECK(spi_bus_initialize(SPI2_HOST,&bus,SPI_DMA_CH_AUTO));
    spi_device_interface_config_t dev={.command_bits=8,.address_bits=24,
        .clock_speed_hz=20000000,.mode=0,.spics_io_num=45,.queue_size=1,
        .flags=SPI_DEVICE_HALFDUPLEX};
    ESP_ERROR_CHECK(spi_bus_add_device(SPI2_HOST,&dev,&lcd));
    dma=heap_caps_malloc(TC_WIDTH*LCD_ROWS*2,MALLOC_CAP_DMA);TC_CHECK(dma);
    command(0x01,NULL,0);vTaskDelay(pdMS_TO_TICKS(120));
    for(size_t i=0;i<sizeof(lcd_init)/sizeof(lcd_init[0]);i++) {
        if(lcd_init[i].delay_ms) vTaskDelay(pdMS_TO_TICKS(lcd_init[i].delay_ms));
        else command(lcd_init[i].command,&lcd_init[i].value,1);
    }
    // This panel requires inversion OFF: ON produces a photographic negative.
    panel_orientation();
    uint8_t *blank=heap_caps_calloc(1,TC_FRAME_BYTES,MALLOC_CAP_SPIRAM);
    TC_CHECK(blank);
    // Visible boot indicator while waiting for the desktop to mirror a frame.
    for(int y=128;y<144;y++) for(int x=100;x<380;x++) {
        int p=(y*TC_WIDTH+x)*2;blank[p]=0x1f;blank[p+1]=0x04;
    }
    board_present(blank);free(blank);gpio_set_level(1,1);
    ESP_LOGI("LCD","NV3041A QSPI ready 480x272");
    i2c_master_bus_handle_t i2c;
    i2c_master_bus_config_t cfg={.i2c_port=I2C_NUM_0,.sda_io_num=8,.scl_io_num=4,
        .clk_source=I2C_CLK_SRC_DEFAULT,.glitch_ignore_cnt=7,.flags.enable_internal_pullup=true};
    ESP_ERROR_CHECK(i2c_new_master_bus(&cfg,&i2c));
    uint8_t addr=0x5d;
    if(i2c_master_probe(i2c,addr,100)!=ESP_OK) addr=0x14;
    ESP_ERROR_CHECK(i2c_master_probe(i2c,addr,100));
    esp_lcd_panel_io_i2c_config_t io_cfg=ESP_LCD_TOUCH_IO_I2C_GT911_CONFIG();
    io_cfg.dev_addr=addr;io_cfg.scl_speed_hz=100000;
    esp_lcd_panel_io_handle_t io;
    ESP_ERROR_CHECK(esp_lcd_new_panel_io_i2c(i2c,&io_cfg,&io));
    esp_lcd_touch_config_t tc={.x_max=TC_WIDTH,.y_max=TC_HEIGHT,.rst_gpio_num=-1,.int_gpio_num=-1};
    ESP_ERROR_CHECK(esp_lcd_touch_new_i2c_gt911(io,&tc,&touch));
    ESP_LOGI("TOUCH","GT911 ready address=0x%02x",addr);
    screen.last_touch=esp_log_timestamp();
    TC_CHECK(xTaskCreate(touch_task,"touch",4096,NULL,3,NULL)==pdPASS);
}
#else
void board_init(void) {}
void board_present(const uint8_t *frame) {(void)frame;}
bool board_input(uint8_t out[13]) {(void)out;return false;}
void board_clear_input(void) {}
#endif
