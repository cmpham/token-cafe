#pragma once
#include <stdbool.h>
#include <stdint.h>
void board_init(void);
void board_present(const uint8_t *frame);
bool board_input(uint8_t out[13]);
void board_clear_input(void);

bool board_test_sample(unsigned id,unsigned count,unsigned x,unsigned y);

void board_backlight(bool on);

void board_present_feedback(const uint8_t *frame,int x,int y);
