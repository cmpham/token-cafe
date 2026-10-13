#include "check.h"
#include <string.h>
#include "protocol.h"
#include "board.h"
#include "pairing_key.h"
#include "esp_random.h"
#include "mbedtls/md.h"
#include "esp_heap_caps.h"
#include "esp_psram.h"
#include "esp_timer.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"
#if CONFIG_TC_QA_USB
#include <stdio.h>
#include <stdlib.h>
#endif
#include "nvs_flash.h"
#include "nimble/nimble_port.h"
#include "nimble/nimble_port_freertos.h"
#include "host/ble_hs.h"
#include "services/gap/ble_svc_gap.h"
#include "services/gatt/ble_svc_gatt.h"

// 7e57cafe-0001-4000-8000-000000000001 (service), ...0002 (RX), ...0003 (TX)
#define UUID(n) BLE_UUID128_INIT(n,0,0,0,0,0,0,0x80,0,0x40,1,0,0xfe,0xca,0x57,0x7e)
static const ble_uuid128_t service_uuid = UUID(1), rx_uuid = UUID(2), tx_uuid = UUID(3);
static const ble_uuid128_t auth_uuid = UUID(4);
static bool authenticated, challenge_pending;
static uint8_t challenge[32];
static void auth_tag(const char *role, uint8_t out[32]) {
    uint8_t input[37];
    size_t len = strlen(role);
    memcpy(input, role, len);
    memcpy(input + len, challenge, sizeof(challenge));
    TC_CHECK(mbedtls_md_hmac(mbedtls_md_info_from_type(MBEDTLS_MD_SHA256),
           pairing_key, sizeof(pairing_key), input, len + sizeof(challenge), out) == 0);
}
static int access_auth(uint16_t handle, uint16_t attr, struct ble_gatt_access_ctxt *ctxt, void *arg) {
    (void)handle; (void)attr; (void)arg;
    uint8_t tag[32];
    if (ctxt->op == BLE_GATT_ACCESS_OP_READ_CHR) {
        if (authenticated) {
            uint16_t size = ble_att_mtu(handle) - 3;
            uint8_t caps[2] = {size & 255, size >> 8};
            return os_mbuf_append(ctxt->om, caps, sizeof(caps)) ? BLE_ATT_ERR_INSUFFICIENT_RES : 0;
        }
        if (!challenge_pending) return BLE_ATT_ERR_UNLIKELY;
        auth_tag("board", tag);
        if (os_mbuf_append(ctxt->om, challenge + 16, 16) || os_mbuf_append(ctxt->om, tag, 16))
            return BLE_ATT_ERR_INSUFFICIENT_RES;
        return 0;
    }
    uint8_t data[16];
    if (OS_MBUF_PKTLEN(ctxt->om) != sizeof(data) || ble_hs_mbuf_to_flat(ctxt->om, data, sizeof(data), NULL))
        return BLE_ATT_ERR_INVALID_ATTR_VALUE_LEN;
    if (!challenge_pending) {
        authenticated = false;
        memcpy(challenge, data, 16);
        esp_fill_random(challenge + 16, 16);
        challenge_pending = true;
    } else {
        auth_tag("host", tag);
        uint8_t diff = 0;
        for (int i = 0; i < 16; i++) diff |= data[i] ^ tag[i];
        challenge_pending = false;
        authenticated = diff == 0;
        if (!authenticated) return BLE_ATT_ERR_INSUFFICIENT_AUTHEN;
        board_clear_input();
        ESP_LOGI("BLE", "AUTHENTICATED companion");
    }
    return 0;
}
static tc_protocol protocol;
static uint8_t own_type;
static uint16_t conn = BLE_HS_CONN_HANDLE_NONE, tx_handle;
static int64_t last_byte;
static uint32_t frame_number;
static uint8_t *display_frame;
#if CONFIG_TC_QA_USB
static SemaphoreHandle_t display_lock;
static uint32_t display_ms;
#endif
static char name[24];
static void advertise(void);

static void transmit(const uint8_t *data, size_t length) {
    if (conn == BLE_HS_CONN_HANDLE_NONE) return;
    // Fragment replies too: default MTU 23 only allows 20-byte notifications.
    size_t max = ble_att_mtu(conn) - 3;
    while (length) {
        size_t n = length < max ? length : max;
        struct os_mbuf *om = ble_hs_mbuf_from_flat(data, n);
        if (!om) { ESP_LOGE("BLE", "notification allocation failed"); return; }
        int rc = ble_gatts_notify_custom(conn, tx_handle, om);
        if (rc) { ESP_LOGE("BLE", "notification failed %d", rc); return; }
        data += n; length -= n;
    }
}
static void restore(uint8_t *frame) { memcpy(frame, display_frame, TC_FRAME_BYTES); }
static void present(const uint8_t *frame) {
    // Present a complete, committed frame on the physical panel.
#if CONFIG_TC_QA_USB
    xSemaphoreTake(display_lock,portMAX_DELAY);
#endif
    memcpy(display_frame, frame, TC_FRAME_BYTES);
    board_present(display_frame);
#if CONFIG_TC_QA_USB
    display_ms=esp_log_timestamp();
#endif
    ESP_LOGI("BLE", "FRAME %lu CRC=%08lx", (unsigned long)++frame_number,
             (unsigned long)tc_crc32(0, display_frame, TC_FRAME_BYTES));
#if CONFIG_TC_QA_USB
    xSemaphoreGive(display_lock);
#endif
}
static int access_rx(uint16_t handle, uint16_t attr, struct ble_gatt_access_ctxt *ctxt, void *arg) {
    (void)handle; (void)attr; (void)arg;
    if (!authenticated) return BLE_ATT_ERR_INSUFFICIENT_AUTHEN;
    if (ctxt->op != BLE_GATT_ACCESS_OP_WRITE_CHR) return BLE_ATT_ERR_UNLIKELY;
    uint16_t len = OS_MBUF_PKTLEN(ctxt->om);
    uint8_t data[512];
    if (len > sizeof(data) || ble_hs_mbuf_to_flat(ctxt->om, data, sizeof(data), NULL))
        return BLE_ATT_ERR_INVALID_ATTR_VALUE_LEN;
    int64_t now = esp_timer_get_time();
    if (now - last_byte > 5000000) tc_reset(&protocol);
    last_byte = now;
    tc_feed(&protocol, data, len);
    return 0;
}
static const struct ble_gatt_svc_def services[] = {
    {.type = BLE_GATT_SVC_TYPE_PRIMARY, .uuid = &service_uuid.u,
     .characteristics = (struct ble_gatt_chr_def[]) {
         {.uuid = &auth_uuid.u, .access_cb = access_auth, .flags = BLE_GATT_CHR_F_READ | BLE_GATT_CHR_F_WRITE},
         {.uuid = &rx_uuid.u, .access_cb = access_rx, .flags = BLE_GATT_CHR_F_WRITE | BLE_GATT_CHR_F_WRITE_NO_RSP},
         {.uuid = &tx_uuid.u, .access_cb = access_rx, .flags = BLE_GATT_CHR_F_NOTIFY, .val_handle = &tx_handle},
         {0},
     }}, {0},
};
static int gap(struct ble_gap_event *event, void *arg) {
    (void)arg;
    switch (event->type) {
    case BLE_GAP_EVENT_CONNECT:
        if (event->connect.status) { advertise(); break; }
        conn = event->connect.conn_handle;
        authenticated = false; challenge_pending = false;
        tc_reset(&protocol);
        board_clear_input();
        ESP_LOGI("BLE", "CONNECTED");
        // Ask for interactive latency; the central may choose a slower interval.
        struct ble_gap_upd_params params = {
            .itvl_min = 12, .itvl_max = 24, .latency = 0, .supervision_timeout = 400,
        };
        ESP_LOGI("BLE", "interval request rc=%d", ble_gap_update_params(conn, &params));
        break;
    case BLE_GAP_EVENT_CONN_UPDATE: {
        struct ble_gap_conn_desc desc;
        if (ble_gap_conn_find(event->conn_update.conn_handle, &desc) == 0) {
            ESP_LOGI("BLE", "interval=%u x1.25ms latency=%u status=%d",
                     desc.conn_itvl, desc.conn_latency, event->conn_update.status);
        }
        break;
    }
    case BLE_GAP_EVENT_DISCONNECT:
        conn = BLE_HS_CONN_HANDLE_NONE;
        authenticated = false; challenge_pending = false;
        tc_reset(&protocol);
        board_clear_input();
        ESP_LOGI("BLE", "DISCONNECTED reason=%d", event->disconnect.reason);
        advertise(); break;
    case BLE_GAP_EVENT_ADV_COMPLETE: advertise(); break;
    default: break;
    }
    return 0;
}
static void advertise(void) {
    struct ble_hs_adv_fields fields = {0};
    fields.flags = BLE_HS_ADV_F_DISC_GEN | BLE_HS_ADV_F_BREDR_UNSUP;
    fields.uuids128 = (ble_uuid128_t *)&service_uuid;
    fields.num_uuids128 = 1;
    fields.uuids128_is_complete = 1;
    TC_CHECK(ble_gap_adv_set_fields(&fields) == 0);
    struct ble_hs_adv_fields response = {0};
    response.name = (uint8_t *)name;
    response.name_len = strlen(name);
    response.name_is_complete = 1;
    TC_CHECK(ble_gap_adv_rsp_set_fields(&response) == 0);
    struct ble_gap_adv_params params = {0};
    params.conn_mode = BLE_GAP_CONN_MODE_UND;
    params.disc_mode = BLE_GAP_DISC_MODE_GEN;
    TC_CHECK(ble_gap_adv_start(own_type, NULL, BLE_HS_FOREVER, &params, gap, NULL) == 0);
    ESP_LOGI("BLE", "ADVERTISING %s", name);
}
static void sync_host(void) {
    TC_CHECK(ble_hs_id_infer_auto(0, &own_type) == 0);
    uint8_t addr[6];
    TC_CHECK(ble_hs_id_copy_addr(own_type, addr, NULL) == 0);
    snprintf(name, sizeof(name), "TokenCafe-%02X%02X", addr[1], addr[0]);
    ble_svc_gap_device_name_set(name);
    advertise();
}
static void host_task(void *arg) {
    (void)arg;
    nimble_port_run();
    nimble_port_freertos_deinit();
}
#if CONFIG_TC_QA_USB
// USB diagnostics never carry live scene updates; those continue over authenticated BLE.
static void qa_task(void *arg) {
    (void)arg;
    uint8_t *capture=heap_caps_malloc(TC_FRAME_BYTES,MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);
    TC_CHECK(capture);
    char line[96];size_t used=0;bool overflow=false;
    for(;;) {
        int ch=getchar();
        if(ch==EOF) { clearerr(stdin);vTaskDelay(pdMS_TO_TICKS(10));continue; }
        if(ch!='\n') {
            if(used+1<sizeof(line)) line[used++]=(char)ch; else overflow=true;
            continue;
        }
        line[used]=0;used=0;
        unsigned id=0,count=0,x=0,y=0;char extra;
        if(overflow) { overflow=false;printf("@QA ERROR overflow\n");continue; }
        if(sscanf(line,"QA IN %u %u %u %u %c",&id,&count,&x,&y,&extra)==4 &&
           count<=2 && x<=65535 && y<=65535) {
            if(!board_test_sample(id,count,x,y)) printf("@QA ERROR queue\n");
        } else if(sscanf(line,"QA IDLE %u %u %c",&id,&x,&extra)==2 && x<=600000) {
            if(!board_test_sample(id,253,x,0)) printf("@QA ERROR queue\n");
        } else if(sscanf(line,"QA MODE %u %u %c",&id,&count,&extra)==2 && count<=1) {
            if(!board_test_sample(id,count?255:254,0,0)) printf("@QA ERROR queue\n");
        } else if(sscanf(line,"QA SNAP %u %c",&id,&extra)==1) {
            xSemaphoreTake(display_lock,portMAX_DELAY);
            memcpy(capture,display_frame,TC_FRAME_BYTES);
            uint32_t frame=frame_number,ms=display_ms;
            xSemaphoreGive(display_lock);
            uint32_t crc=tc_crc32(0,capture,TC_FRAME_BYTES);
            printf("@QA SNAP %u %lu %lu %08lx %u\n",id,(unsigned long)frame,(unsigned long)ms,(unsigned long)crc,TC_FRAME_BYTES);
            static const char digits[]="0123456789abcdef";
            for(size_t offset=0;offset<TC_FRAME_BYTES;offset+=128) {
                size_t n=TC_FRAME_BYTES-offset;if(n>128)n=128;
                char hex[257];
                for(size_t i=0;i<n;i++){hex[i*2]=digits[capture[offset+i]>>4];hex[i*2+1]=digits[capture[offset+i]&15];}
                hex[n*2]=0;printf("@QA DATA %u %u %s\n",id,(unsigned)offset,hex);
            }
            printf("@QA END %u\n",id);
        } else printf("@QA ERROR command\n");
    }
}
#endif
void app_main(void) {
    esp_log_level_set("NimBLE", ESP_LOG_WARN);
    esp_err_t nvs = nvs_flash_init();
    if (nvs == ESP_ERR_NVS_NO_FREE_PAGES || nvs == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase()); nvs = nvs_flash_init();
    }
    ESP_ERROR_CHECK(nvs);
    TC_CHECK(esp_psram_get_size() == 8 * 1024 * 1024);
    uint8_t *frame = heap_caps_malloc(TC_FRAME_BYTES, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    display_frame = heap_caps_calloc(1, TC_FRAME_BYTES, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    TC_CHECK(frame && display_frame);
    tc_init(&protocol, frame, transmit, present);
    protocol.restore=restore;
    protocol.scratch=heap_caps_malloc(TC_FRAME_BYTES,MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    TC_CHECK(protocol.scratch);
#if CONFIG_TC_QA_USB
    display_lock=xSemaphoreCreateMutex();TC_CHECK(display_lock);
#endif
    board_init();
#if CONFIG_TC_QA_USB
    TC_CHECK(xTaskCreate(qa_task,"usb_qa",6144,NULL,2,NULL)==pdPASS);
#endif
    protocol.input = board_input;
    ESP_ERROR_CHECK(nimble_port_init());
    ble_svc_gap_init();
    ble_svc_gatt_init();
    TC_CHECK(ble_gatts_count_cfg(services) == 0);
    TC_CHECK(ble_gatts_add_svcs(services) == 0);
    ble_hs_cfg.sync_cb = sync_host;
    nimble_port_freertos_init(host_task);
}
