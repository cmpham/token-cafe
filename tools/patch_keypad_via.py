"""Route our reserved raw-HID messages before VIA consumes unknown commands."""
from pathlib import Path
import sys

p = Path(sys.argv[1])
s = p.read_text()
marker = "    /* Token Cafe raw HID routing */"
if marker not in s:
    anchor = "    memset(response_report, 0, sizeof(response_report));"
    start = s.index("static int via_raw_hid_received_listener(")
    pos = s.index(anchor, start)
    s = s[:pos] + '''    /* Token Cafe raw HID routing */
#if IS_ENABLED(CONFIG_TOKEN_CAFE_KEYPAD)
    extern bool token_cafe_raw_hid(const struct raw_hid_received_event *);
    if (token_cafe_raw_hid(received)) return ZMK_EV_EVENT_HANDLED;
#endif

''' + s[pos:]
    p.write_text(s)
