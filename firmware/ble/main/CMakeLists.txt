idf_component_register(SRCS "main.c" "../../main/protocol.c" INCLUDE_DIRS "../../main" REQUIRES mbedtls bt nvs_flash esp_psram esp_timer)
