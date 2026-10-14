// QEMU-only deterministic IO adapter. Never included in production firmware.
#include "engine.h"
#include "driver/uart.h"
#include "esp_heap_caps.h"
#include "esp_random.h"
#include "esp_lcd_panel_ops.h"
#include "esp_lcd_qemu_rgb.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include <assert.h>
#include <string.h>
#include <stdlib.h>
static Engine *engine;
static esp_lcd_panel_handle_t panel;
static uint16_t *frame;
static uint64_t now;
static uint8_t journal[4096];
static size_t journal_len;
void *cafe_alloc(size_t size,size_t align){return heap_caps_aligned_alloc(align<4?4:align,size,MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);}
void cafe_free(void *ptr){heap_caps_free(ptr);}
void cafe_random(uint8_t *ptr,size_t len){esp_fill_random(ptr,len);}
bool cafe_persist(const uint8_t *ptr,size_t len){if(len>sizeof(journal))return false;memcpy(journal,ptr,len);journal_len=len;return true;}
static void reset(unsigned capacity){if(engine)cafe_engine_drop(engine);uint8_t key[32];memset(key,0x42,sizeof(key));engine=cafe_engine_new((const uint8_t *)"qa-qemu",7,key,capacity,true);if(engine&&journal_len)assert(cafe_engine_journal(engine,journal,journal_len));now=0;}
static void read_exact(uint8_t *buf,size_t len){while(len){int n=uart_read_bytes(UART_NUM_0,buf,len,pdMS_TO_TICKS(20));if(n>0){buf+=n;len-=n;}}}
static void reply(uint8_t op,bool ok,const uint8_t *body,size_t len){uint8_t h[7]={'Q','V','2',op,ok?0:1,(uint8_t)len,(uint8_t)(len>>8)};uart_write_bytes(UART_NUM_0,h,sizeof(h));if(len)uart_write_bytes(UART_NUM_0,body,len);}
void app_main(void){
 uart_config_t u={.baud_rate=115200,.data_bits=UART_DATA_8_BITS,.parity=UART_PARITY_DISABLE,.stop_bits=UART_STOP_BITS_1,.flow_ctrl=UART_HW_FLOWCTRL_DISABLE,.source_clk=UART_SCLK_DEFAULT};
 ESP_ERROR_CHECK(uart_param_config(UART_NUM_0,&u));ESP_ERROR_CHECK(uart_driver_install(UART_NUM_0,8192,0,0,NULL,0));
 esp_lcd_rgb_qemu_config_t config={.width=480,.height=272,.bpp=RGB_QEMU_BPP_16};
 ESP_ERROR_CHECK(esp_lcd_new_rgb_qemu(&config,&panel));ESP_ERROR_CHECK(esp_lcd_panel_init(panel));ESP_ERROR_CHECK(esp_lcd_rgb_qemu_get_frame_buffer(panel,(void **)&frame));
 memset(frame,0,480*272*2);ESP_ERROR_CHECK(esp_lcd_rgb_qemu_refresh(panel));reset(182);assert(engine);uart_write_bytes(UART_NUM_0,"TC_V2_READY\n",12);
 uint8_t input[1024],out[1024];
 while(1){uint8_t header[6];read_exact(header,6);if(memcmp(header,"QV2",3))abort();unsigned size=header[4]|header[5]<<8;if(size>sizeof(input))abort();read_exact(input,size);bool ok=true;size_t n=0;
 switch(header[3]){
 case 1:n=cafe_engine_auth_read(engine,out,sizeof(out));ok=n>0;break;
 case 2:ok=cafe_engine_auth_write(engine,input,size,now);break;
 case 3:ok=cafe_engine_receive(engine,input,size,now);break;
 case 4:n=cafe_engine_next(engine,out,sizeof(out));break;
 case 5:if(size!=8){ok=false;break;}memcpy(&now,input,8);ok=cafe_engine_tick(engine,now);break;
 case 6:if(size!=5){ok=false;break;}ok=cafe_engine_touch(engine,input[0],input[1]|input[2]<<8,input[3]|input[4]<<8,now);break;
 case 7:ok=cafe_engine_render(engine,frame,now);if(ok){ESP_ERROR_CHECK(esp_lcd_rgb_qemu_refresh(panel));cafe_engine_presented(engine,now);}break;
 case 8:{uint32_t status=cafe_engine_status(engine);memcpy(out,&status,4);n=4;break;}
 case 9:if(size!=2){ok=false;break;}reset(input[0]|input[1]<<8);ok=engine!=NULL;break;
 case 10:if(size!=4){ok=false;break;}uint32_t offset;memcpy(&offset,input,4);if(offset>=480*272*2){ok=false;break;}n=480*272*2-offset;if(n>1024)n=1024;memcpy(out,(uint8_t *)frame+offset,n);break;
 default:ok=false;break;
 }reply(header[3],ok,out,n);
 }
}
