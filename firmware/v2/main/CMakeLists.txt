idf_component_register(SRCS "main.c" INCLUDE_DIRS "../../shared"
    REQUIRES esp_driver_uart esp_lcd esp_psram esp_timer esp_hw_support)
add_prebuilt_library(cafe_engine "../../engine/target/xtensa-esp32s3-none-elf/release/libcafe_board_engine.a")
target_link_libraries(${COMPONENT_LIB} PRIVATE cafe_engine m)
