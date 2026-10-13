#pragma once
#include <stdbool.h>
#include <stdint.h>
void board_init(void);
void board_present(const uint8_t *frame);
bool board_input(uint8_t out[5]);
void board_clear_input(void);

bool board_test_sample(unsigned id,unsigned count,unsigned x,unsigned y);
