/* SPDX-License-Identifier: MIT
 * From K4zoku/ZMK-VIA at 9f8796671ae5843277e0daaaf711819499ad4a90.
 * See ../LICENSE-MIT.
 */
#include <string.h>

#include <via_custom_keycode.h>
#include <dt-bindings/zmk/bt.h>

#define PIPAR_CUSTOM_KEYCODE_BASE 0x7E00
#define PIPAR_CUSTOM_KEYCODE_LAST 0x7E03

static bool pipar_to_binding(uint16_t keycode, struct zmk_behavior_binding *binding) {
    if (keycode < PIPAR_CUSTOM_KEYCODE_BASE || keycode > PIPAR_CUSTOM_KEYCODE_LAST) {
        return false;
    }

    switch (keycode - PIPAR_CUSTOM_KEYCODE_BASE) {
    case 0:
        *binding = (struct zmk_behavior_binding){
            .behavior_dev = DEVICE_DT_NAME(DT_NODELABEL(bt)),
            .param1 = BT_NXT_CMD,
        };
        return true;
    case 1:
        *binding = (struct zmk_behavior_binding){
            .behavior_dev = DEVICE_DT_NAME(DT_NODELABEL(bt)),
            .param1 = BT_CLR_CMD,
        };
        return true;
    case 2:
        *binding = (struct zmk_behavior_binding){
            .behavior_dev = DEVICE_DT_NAME(DT_NODELABEL(sys_reset)),
        };
        return true;
    case 3:
        *binding = (struct zmk_behavior_binding){
            .behavior_dev = DEVICE_DT_NAME(DT_NODELABEL(bootloader)),
        };
        return true;
    default:
        return false;
    }
}

static bool pipar_from_binding(const struct zmk_behavior_binding *binding, uint16_t *keycode) {
    if (!binding || !binding->behavior_dev) {
        return false;
    }

    if (strcmp(binding->behavior_dev, DEVICE_DT_NAME(DT_NODELABEL(bt))) == 0 &&
        binding->param2 == 0) {
        if (binding->param1 == BT_NXT_CMD) {
            *keycode = PIPAR_CUSTOM_KEYCODE_BASE;
            return true;
        }
        if (binding->param1 == BT_CLR_CMD) {
            *keycode = PIPAR_CUSTOM_KEYCODE_BASE + 1;
            return true;
        }
    }
    if (strcmp(binding->behavior_dev, DEVICE_DT_NAME(DT_NODELABEL(sys_reset))) == 0 &&
        binding->param1 == 0 && binding->param2 == 0) {
        *keycode = PIPAR_CUSTOM_KEYCODE_BASE + 2;
        return true;
    }
    if (strcmp(binding->behavior_dev, DEVICE_DT_NAME(DT_NODELABEL(bootloader))) == 0 &&
        binding->param1 == 0 && binding->param2 == 0) {
        *keycode = PIPAR_CUSTOM_KEYCODE_BASE + 3;
        return true;
    }
    return false;
}

ZMK_VIA_CUSTOM_KEYCODE_PROVIDER_DEFINE(pipar_custom_keycodes, PIPAR_CUSTOM_KEYCODE_BASE,
                                        PIPAR_CUSTOM_KEYCODE_LAST, pipar_to_binding,
                                        pipar_from_binding);
