/* SPDX-License-Identifier: MIT */
#ifndef TOKEN_CAFE_KEYPAD_TRANSPORT_H
#define TOKEN_CAFE_KEYPAD_TRANSPORT_H
#include <stdint.h>
uint32_t token_cafe_input_generation(void);
/* Caller supplies fallback text; replaced only by a complete or expired frame. */
void token_cafe_display_lines(char lines[4][17]);
#endif
