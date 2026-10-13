#include "protocol.h"
#include "driver/uart.h"
#include "esp_heap_caps.h"
#include "sdkconfig.h"
#if !CONFIG_TC_HEADLESS
#include "esp_lcd_panel_ops.h"
#include "esp_lcd_qemu_rgb.h"
#endif
#include "esp_psram.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include <assert.h>
#include <string.h>

#if !CONFIG_TC_HEADLESS
static esp_lcd_panel_handle_t panel;
static void *panel_frame;
#endif
static tc_protocol protocol;

static void transmit(const uint8_t *data, size_t size) {
    uart_write_bytes(UART_NUM_0, data, size);
}

static void present(const uint8_t *frame) {
    #if CONFIG_TC_HEADLESS
    (void)frame; // Physical bring-up: validate frame storage without an attached LCD.
    #else
    // QEMU's panel cannot DMA directly from the PSRAM virtual address range.
    memcpy(panel_frame, frame, TC_FRAME_BYTES);
    ESP_ERROR_CHECK(esp_lcd_rgb_qemu_refresh(panel));
    #endif
}

void app_main(void) {
    // Hardware build uses UART0 via the COM connector and no display GPIOs.
    assert(esp_psram_get_size() == 8 * 1024 * 1024);
    uint8_t *frame = heap_caps_malloc(TC_FRAME_BYTES, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    assert(frame);
    #if !CONFIG_TC_HEADLESS
    esp_lcd_rgb_qemu_config_t config = {
        .width = TC_WIDTH, .height = TC_HEIGHT, .bpp = RGB_QEMU_BPP_16,
    };
    ESP_ERROR_CHECK(esp_lcd_new_rgb_qemu(&config, &panel));
    ESP_ERROR_CHECK(esp_lcd_panel_init(panel));
    ESP_ERROR_CHECK(esp_lcd_rgb_qemu_get_frame_buffer(panel, &panel_frame));
    #endif
    uart_config_t uart = {
        .baud_rate = 115200, .data_bits = UART_DATA_8_BITS,
        .parity = UART_PARITY_DISABLE, .stop_bits = UART_STOP_BITS_1,
        .flow_ctrl = UART_HW_FLOWCTRL_DISABLE, .source_clk = UART_SCLK_DEFAULT,
    };
    ESP_ERROR_CHECK(uart_param_config(UART_NUM_0, &uart));
    ESP_ERROR_CHECK(uart_driver_install(UART_NUM_0, 8192, 0, 0, NULL, 0));
    tc_init(&protocol, frame, transmit, present);
    present(frame);
    transmit((const uint8_t *)"TC_READY\n", 9);
    uint8_t input[256];
    int64_t last_byte = esp_timer_get_time();
    while (1) {
        int n = uart_read_bytes(UART_NUM_0, input, sizeof(input), pdMS_TO_TICKS(20));
        int64_t now = esp_timer_get_time();
        if (now - last_byte > 1000000) tc_reset(&protocol);
        if (n > 0) {
            tc_feed(&protocol, input, n);
            last_byte = now;
        }
    }
}
