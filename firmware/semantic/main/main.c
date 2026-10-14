#include "engine.h"
#include "board.h"
#include "pairing.h"
#include "check.h"
#include "protocol.h"
#include "esp_heap_caps.h"
#include "esp_random.h"
#include "esp_timer.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"
#include "nvs_flash.h"
#include "nvs.h"
#include "nimble/nimble_port.h"
#include "nimble/nimble_port_freertos.h"
#include "host/ble_hs.h"
#include "services/gap/ble_svc_gap.h"
#include "services/gatt/ble_svc_gatt.h"
#include <string.h>
#include <stdio.h>
#include <stdlib.h>
#if CONFIG_TC_QA_USB
#define TC_INJECTION true
#if !TC_KEY_QA
#error "Injectable firmware must use a QA key"
#endif
#else
#define TC_INJECTION false
#endif
#define UUID(n) BLE_UUID128_INIT(n,0,0,0,0,0,0,0x80,0,0x40,1,0,0xfe,0xca,0x57,0x7e)
static const ble_uuid128_t service_uuid=UUID(1),rx_uuid=UUID(5),tx_uuid=UUID(6),auth_uuid=UUID(7);
static Engine *engine;
static SemaphoreHandle_t lock;
typedef struct { uint16_t len; uint8_t data[509]; } RxPacket;
static QueueHandle_t rx_packets;
static uint8_t own_type;
static uint16_t conn=BLE_HS_CONN_HANDLE_NONE,tx_handle;
static bool subscribed;
static uint16_t *frame;
static uint32_t frame_number,display_ms;
static nvs_handle_t storage;
static void advertise(void);
static uint64_t now_ms(void){return esp_timer_get_time()/1000;}
void *cafe_alloc(size_t size,size_t align){return heap_caps_aligned_alloc(align<4?4:align,size,MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);}
void cafe_free(void *ptr){heap_caps_free(ptr);}
void cafe_random(uint8_t *ptr,size_t len){esp_fill_random(ptr,len);}
bool cafe_persist(const uint8_t *ptr,size_t len){return nvs_set_blob(storage,"receipts",ptr,len)==ESP_OK&&nvs_commit(storage)==ESP_OK;}
static int auth(uint16_t handle,uint16_t attr,struct ble_gatt_access_ctxt *ctxt,void *arg){
 (void)attr;(void)arg;uint8_t data[80];size_t len;bool ok=true;
 xSemaphoreTake(lock,portMAX_DELAY);
 if(ctxt->op==BLE_GATT_ACCESS_OP_READ_CHR){len=cafe_engine_auth_read(engine,data,sizeof(data));ok=len&&os_mbuf_append(ctxt->om,data,len)==0;}
 else{len=OS_MBUF_PKTLEN(ctxt->om);ok=ble_att_mtu(handle)>=185&&len<=sizeof(data)&&ble_hs_mbuf_to_flat(ctxt->om,data,sizeof(data),NULL)==0&&cafe_engine_auth_write(engine,data,len,now_ms());}
 xSemaphoreGive(lock);return ok?0:BLE_ATT_ERR_INSUFFICIENT_AUTHEN;
}
static int receive(uint16_t handle,uint16_t attr,struct ble_gatt_access_ctxt *ctxt,void *arg){
 (void)handle;(void)attr;(void)arg;RxPacket p;size_t len=OS_MBUF_PKTLEN(ctxt->om);
 if(len>sizeof(p.data)||ble_hs_mbuf_to_flat(ctxt->om,p.data,sizeof(p.data),NULL))return BLE_ATT_ERR_INVALID_ATTR_VALUE_LEN;
 p.len=len;
 // ATT acknowledgement confirms bounded receipt, never application success.
 // Decode on the model task so the BLE callback cannot wait for LCD drawing.
 return xQueueSend(rx_packets,&p,0)==pdTRUE?0:BLE_ATT_ERR_INSUFFICIENT_RES;
}
static int read_tx(uint16_t h,uint16_t a,struct ble_gatt_access_ctxt *c,void *p){(void)h;(void)a;(void)c;(void)p;return 0;}
static const struct ble_gatt_svc_def services[]={
 {.type=BLE_GATT_SVC_TYPE_PRIMARY,.uuid=&service_uuid.u,.characteristics=(struct ble_gatt_chr_def[]){
 {.uuid=&rx_uuid.u,.access_cb=receive,.flags=BLE_GATT_CHR_F_WRITE},
 {.uuid=&tx_uuid.u,.access_cb=read_tx,.flags=BLE_GATT_CHR_F_NOTIFY,.val_handle=&tx_handle},
 {.uuid=&auth_uuid.u,.access_cb=auth,.flags=BLE_GATT_CHR_F_READ|BLE_GATT_CHR_F_WRITE},{0}}},{0}};
static int gap(struct ble_gap_event *event,void *arg){(void)arg;
 switch(event->type){
 case BLE_GAP_EVENT_CONNECT:
  if(event->connect.status){advertise();break;}conn=event->connect.conn_handle;subscribed=false;
  {struct ble_gap_upd_params p={.itvl_min=12,.itvl_max=24,.latency=0,.supervision_timeout=400};ble_gap_update_params(conn,&p);}ESP_LOGI("V2","CONNECTED");break;
 case BLE_GAP_EVENT_SUBSCRIBE:if(event->subscribe.attr_handle==tx_handle)subscribed=event->subscribe.cur_notify;break;
 case BLE_GAP_EVENT_DISCONNECT:
  xSemaphoreTake(lock,portMAX_DELAY);conn=BLE_HS_CONN_HANDLE_NONE;subscribed=false;cafe_engine_disconnect(engine);xQueueReset(rx_packets);board_clear_input();xSemaphoreGive(lock);ESP_LOGI("V2","DISCONNECTED %d",event->disconnect.reason);advertise();break;
 case BLE_GAP_EVENT_ADV_COMPLETE:advertise();break;
 default:break;
 }return 0;
}
static void advertise(void){struct ble_hs_adv_fields f={.flags=BLE_HS_ADV_F_DISC_GEN|BLE_HS_ADV_F_BREDR_UNSUP,.uuids128=(ble_uuid128_t *)&service_uuid,.num_uuids128=1,.uuids128_is_complete=1};TC_CHECK(ble_gap_adv_set_fields(&f)==0);struct ble_hs_adv_fields r={.name=(uint8_t *)TC_BOARD_NAME,.name_len=strlen(TC_BOARD_NAME),.name_is_complete=1};TC_CHECK(ble_gap_adv_rsp_set_fields(&r)==0);struct ble_gap_adv_params p={.conn_mode=BLE_GAP_CONN_MODE_UND,.disc_mode=BLE_GAP_DISC_MODE_GEN};TC_CHECK(ble_gap_adv_start(own_type,NULL,BLE_HS_FOREVER,&p,gap,NULL)==0);ESP_LOGI("V2","ADVERTISING %s",TC_BOARD_NAME);}
static void sync_host(void){TC_CHECK(ble_hs_id_infer_auto(0,&own_type)==0);ble_svc_gap_device_name_set(TC_BOARD_NAME);advertise();}
static void host_task(void *arg){(void)arg;nimble_port_run();nimble_port_freertos_deinit();}
#if CONFIG_TC_QA_USB
static void qa_task(void *arg){
 (void)arg;uint8_t *capture=cafe_alloc(TC_FRAME_BYTES,4);TC_CHECK(capture);char line[96];size_t used=0;bool overflow=false;
 for(;;){int ch=getchar();if(ch==EOF){clearerr(stdin);vTaskDelay(pdMS_TO_TICKS(10));continue;}if(ch!='\n'){if(used+1<sizeof(line))line[used++]=(char)ch;else overflow=true;continue;}line[used]=0;used=0;unsigned id,count,x,y;char extra;if(overflow){overflow=false;continue;}
  if(sscanf(line,"QA IN %u %u %u %u %c",&id,&count,&x,&y,&extra)==4&&count<=2&&x<480&&y<272){board_test_sample(id,count,x,y);}
  else if(sscanf(line,"QA MODE %u %u %c",&id,&count,&extra)==2&&count<=1){board_test_sample(id,count?255:254,0,0);}
  else if(sscanf(line,"QA SNAP %u %c",&id,&extra)==1){xSemaphoreTake(lock,portMAX_DELAY);memcpy(capture,frame,TC_FRAME_BYTES);uint32_t n=frame_number,ms=display_ms;xSemaphoreGive(lock);printf("@QA SNAP %u %lu %lu %08lx %u\n",id,(unsigned long)n,(unsigned long)ms,(unsigned long)tc_crc32(0,capture,TC_FRAME_BYTES),TC_FRAME_BYTES);for(size_t off=0;off<TC_FRAME_BYTES;off+=128){size_t n=TC_FRAME_BYTES-off;if(n>128)n=128;char hex[257];const char *digits="0123456789abcdef";for(size_t i=0;i<n;i++){hex[i*2]=digits[capture[off+i]>>4];hex[i*2+1]=digits[capture[off+i]&15];}hex[n*2]=0;printf("@QA DATA %u %u %s\n",id,(unsigned)off,hex);}printf("@QA END %u\n",id);}
 }
}
#endif
void app_main(void){
 ESP_ERROR_CHECK(nvs_flash_init());ESP_ERROR_CHECK(nvs_open("cafe_v2",NVS_READWRITE,&storage));
 lock=xSemaphoreCreateMutex();TC_CHECK(lock);rx_packets=xQueueCreate(32,sizeof(RxPacket));TC_CHECK(rx_packets);frame=cafe_alloc(TC_FRAME_BYTES,4);TC_CHECK(frame);memset(frame,0,TC_FRAME_BYTES);
 engine=cafe_engine_new((const uint8_t *)TC_DEVICE_ID,strlen(TC_DEVICE_ID),pairing_key,182,TC_INJECTION);TC_CHECK(engine);
 size_t size=0;esp_err_t stored=nvs_get_blob(storage,"receipts",NULL,&size);if(stored==ESP_OK){TC_CHECK(size<=4096);uint8_t *b=cafe_alloc(size,4);TC_CHECK(b);ESP_ERROR_CHECK(nvs_get_blob(storage,"receipts",b,&size));TC_CHECK(cafe_engine_journal(engine,b,size));cafe_free(b);}else TC_CHECK(stored==ESP_ERR_NVS_NOT_FOUND);
 board_init();
 #if CONFIG_TC_QA_USB
 TC_CHECK(xTaskCreate(qa_task,"usb_qa",6144,NULL,2,NULL)==pdPASS);
 #endif
 ESP_ERROR_CHECK(nimble_port_init());ble_svc_gap_init();ble_svc_gatt_init();TC_CHECK(ble_gatts_count_cfg(services)==0);TC_CHECK(ble_gatts_add_svcs(services)==0);ble_hs_cfg.sync_cb=sync_host;nimble_port_freertos_init(host_task);
 uint8_t packet[509];size_t pending=0;uint64_t last_render=0;bool asleep=false,feedback_held=false;uint32_t input_ms=0,commit_ms=0;uint8_t input_kind=0;int input_x=0,input_y=0;
 for(;;){xSemaphoreTake(lock,portMAX_DELAY);uint64_t now=now_ms();bool healthy=true;RxPacket rx;for(int received=0;received<8&&xQueueReceive(rx_packets,&rx,0)==pdTRUE;received++){healthy=cafe_engine_receive(engine,rx.data,rx.len,now_ms())&&healthy;}now=now_ms();healthy=cafe_engine_tick(engine,now)&&healthy;uint8_t in[13];while(board_input(in)){uint32_t at;memcpy(&at,in+5,4);now=now_ms();healthy=cafe_engine_touch(engine,in[0],in[1]|in[2]<<8,in[3]|in[4]<<8,now)&&healthy;input_ms=at;input_kind=in[0];input_x=in[1]|in[2]<<8;input_y=in[3]|in[4]<<8;if(input_kind!=1)feedback_held=false;commit_ms=now_ms();ESP_LOGI("V2","TOUCH %u %u,%u queue_ms=%lu",in[0],in[1]|in[2]<<8,in[3]|in[4]<<8,(unsigned long)((uint32_t)now-at));}
  if(input_ms&&input_kind==1){feedback_held=cafe_engine_feedback(engine,frame);if(feedback_held){board_present_feedback((uint8_t *)frame,input_x,input_y);display_ms=now_ms();last_render=display_ms;ESP_LOGI("V2","FEEDBACK input=%lu lcd=%lu",(unsigned long)input_ms,(unsigned long)display_ms);input_ms=0;}}now=now_ms();
  uint32_t state=cafe_engine_status(engine);if(asleep!=((state&4)!=0)){asleep=(state&4)!=0;board_backlight(!asleep);}
  uint64_t render_started=now_ms();if(!asleep&&now-last_render>=(feedback_held?150:33)&&cafe_engine_draw(engine,now)){uint64_t drawn=now_ms();cafe_engine_pack(engine,frame);uint64_t begun=now_ms();board_present((uint8_t *)frame);display_ms=now_ms();cafe_engine_presented(engine,display_ms);frame_number++;last_render=now;if(input_ms){ESP_LOGI("V2","LOCAL kind=%u input=%lu commit=%lu lcd=%lu",input_kind,(unsigned long)input_ms,(unsigned long)commit_ms,(unsigned long)display_ms);input_ms=0;}if(frame_number%300==0)ESP_LOGI("V2","FRAME draw_ms=%lu pack_ms=%lu lcd_ms=%lu heap=%u",(unsigned long)(drawn-render_started),(unsigned long)(begun-drawn),(unsigned long)(now_ms()-begun),(unsigned)heap_caps_get_free_size(MALLOC_CAP_SPIRAM));}
  if(!healthy&&conn!=BLE_HS_CONN_HANDLE_NONE)ble_gap_terminate(conn,BLE_ERR_REM_USER_CONN_TERM);
  if(conn==BLE_HS_CONN_HANDLE_NONE){pending=0;}else if(subscribed){for(int sent=0;sent<8;sent++){if(!pending)pending=cafe_engine_next(engine,packet,sizeof(packet));if(!pending)break;struct os_mbuf *om=ble_hs_mbuf_from_flat(packet,pending);if(!om)break;int rc=ble_gatts_notify_custom(conn,tx_handle,om);if(!rc){pending=0;continue;}if(rc!=BLE_HS_ENOMEM&&rc!=BLE_HS_EBUSY){pending=0;ble_gap_terminate(conn,BLE_ERR_REM_USER_CONN_TERM);}break;}}
  xSemaphoreGive(lock);vTaskDelay(1);
 }
}
