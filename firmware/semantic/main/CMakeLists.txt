idf_component_register(SRCS "main.c" "../../ble/main/board.c" "../../main/protocol.c"
 INCLUDE_DIRS "../../shared" "../../ble/main" "../../main"
 REQUIRES bt nvs_flash esp_psram esp_timer esp_driver_spi esp_driver_gpio esp_driver_i2c esp_lcd mbedtls)
target_compile_definitions(${COMPONENT_LIB} PRIVATE TC_SEMANTIC=1)
add_prebuilt_library(cafe_engine "../../engine/target/xtensa-esp32s3-none-elf/release/libcafe_board_engine.a")
target_link_libraries(${COMPONENT_LIB} PRIVATE cafe_engine m)
