idf_component_register(SRCS "main.c" "protocol.c" INCLUDE_DIRS "."
    REQUIRES esp_driver_uart esp_lcd esp_psram esp_timer)
