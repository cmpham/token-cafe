#pragma once
#include <stdlib.h>
#include "esp_log.h"
// Initialization and HMAC calls must run even when assertions are disabled.
#define TC_CHECK(condition) do { \
    if (!(condition)) { ESP_LOGE("CHECK", "%s", #condition); abort(); } \
} while (0)
