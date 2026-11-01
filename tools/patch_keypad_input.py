"""Route dedicated keypad controls before ZMK emits ordinary keyboard reports."""
from pathlib import Path
import sys
p = Path(sys.argv[1])
s = p.read_text()
marker = '    /* Token Cafe dedicated USB controls */'
if marker not in s:
    anchor = '    if (ev) {'
    start = s.index('int hid_listener(')
    pos = s.index(anchor, start)
    s = s[:pos] + '''    /* Token Cafe dedicated USB controls */
#if IS_ENABLED(CONFIG_TOKEN_CAFE_KEYPAD)
    extern bool token_cafe_usb_keycode(const struct zmk_keycode_state_changed *);
    if (ev && token_cafe_usb_keycode(ev)) return ZMK_EV_EVENT_HANDLED;
#endif
''' + s[pos:]
    p.write_text(s)

# Advertise only the dedicated Mac service, not a keyboard/board OLED service.
p = Path(sys.argv[2])
s = p.read_text()
if '/* Token Cafe Mac advertising */' not in s:
    start = s.index('static struct bt_data zmk_ble_ad[] = {')
    end = s.index('};', start) + 2
    s = s[:start] + """/* Token Cafe Mac advertising */
static struct bt_data zmk_ble_ad[] = {
    BT_DATA_BYTES(BT_DATA_FLAGS, (BT_LE_AD_GENERAL | BT_LE_AD_NO_BREDR)),
    BT_DATA_BYTES(BT_DATA_UUID128_ALL, 0x01,0xe6,0x8d,0x1d,0x38,0x58,0x7d,0x9b,
                  0xd9,0x4c,0x67,0x7f,0x10,0xfe,0xca,0xf6),
};""" + s[end:]
    p.write_text(s)
