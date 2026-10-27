#include <stdio.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_log.h"
#include "esp_intr_alloc.h"
#include "usb/usb_host.h"

static usb_host_client_handle_t client;
static usb_device_handle_t device;
static uint8_t pending_address;
static bool gone;

static void event(const usb_host_client_event_msg_t *msg, void *arg) {
    (void)arg;
    if (msg->event == USB_HOST_CLIENT_EVENT_NEW_DEV) pending_address = msg->new_dev.address;
    if (msg->event == USB_HOST_CLIENT_EVENT_DEV_GONE) gone = true;
}
static void daemon(void *arg) {
    (void)arg;
    while (1) {
        uint32_t flags;
        ESP_ERROR_CHECK(usb_host_lib_handle_events(portMAX_DELAY, &flags));
    }
}
static void describe(void) {
    const usb_device_desc_t *desc;
    const usb_config_desc_t *config;
    usb_device_info_t info;
    ESP_ERROR_CHECK(usb_host_get_device_descriptor(device, &desc));
    ESP_ERROR_CHECK(usb_host_get_active_config_descriptor(device, &config));
    ESP_ERROR_CHECK(usb_host_device_info(device, &info));
    printf("PROBE_CONNECTED VID=%04x PID=%04x speed=%d\n", desc->idVendor, desc->idProduct, info.speed);
    usb_print_device_descriptor(desc);
    usb_print_config_descriptor(config, NULL);
    if (info.str_desc_manufacturer) usb_print_string_descriptor(info.str_desc_manufacturer);
    if (info.str_desc_product) usb_print_string_descriptor(info.str_desc_product);
    // Include class-specific descriptors so the host can inspect UAC version,
    // terminal directions, PCM formats, sample rates and endpoint capabilities.
    const uint8_t *raw = (const uint8_t *)config;
    for (size_t offset = 0; offset + 2 <= config->wTotalLength;) {
        size_t length = raw[offset];
        if (length < 2 || offset + length > config->wTotalLength) break;
        printf("DESC");
        for (size_t i = 0; i < length; i++) printf(" %02x", raw[offset + i]);
        printf("\n");
        offset += length;
    }
    printf("PROBE_DESCRIPTORS_DONE (no audio playback or microphone capture)\n");
    fflush(stdout);
}
void app_main(void) {
    usb_host_config_t host = {.skip_phy_setup = false, .intr_flags = ESP_INTR_FLAG_LEVEL1};
    ESP_ERROR_CHECK(usb_host_install(&host));
    usb_host_client_config_t cfg = {
        .is_synchronous = false, .max_num_event_msg = 5,
        .async = {.client_event_callback = event, .callback_arg = NULL},
    };
    ESP_ERROR_CHECK(usb_host_client_register(&cfg, &client));
    assert(xTaskCreate(daemon, "usb_events", 4096, NULL, 2, NULL) == pdPASS);
    printf("PROBE_READY: USB host active; waiting for headset\n");
    int ticks = 0;
    while (1) {
        esp_err_t result = usb_host_client_handle_events(client, pdMS_TO_TICKS(1000));
        if (result != ESP_OK && result != ESP_ERR_TIMEOUT) ESP_ERROR_CHECK(result);
        if (gone) {
            if (device) ESP_ERROR_CHECK(usb_host_device_close(client, device));
            device = NULL;
            gone = false;
            printf("PROBE_DISCONNECTED\n");
        }
        if (pending_address) {
            if (!device) {
                esp_err_t err = usb_host_device_open(client, pending_address, &device);
                if (err == ESP_OK) describe();
                else ESP_LOGE("probe", "open: %s", esp_err_to_name(err));
            }
            pending_address = 0;
        }
        if (++ticks % 5 == 0) {
            printf("PROBE_STATUS: %s\n", device ? "device present" : "no device enumerated");
            fflush(stdout);
        }
    }
}
