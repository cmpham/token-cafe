cmake_minimum_required(VERSION 3.22.1)
project(token_cafe_jni C)
add_library(token_cafe_android SHARED IMPORTED)
set_target_properties(token_cafe_android PROPERTIES IMPORTED_LOCATION
    "${CMAKE_CURRENT_SOURCE_DIR}/../../../../native/${ANDROID_ABI}/libtoken_cafe_android.so")
add_library(token_cafe_jni SHARED bridge.c)
target_compile_options(token_cafe_jni PRIVATE -Wall -Wextra -Werror)
target_link_libraries(token_cafe_jni token_cafe_android)
