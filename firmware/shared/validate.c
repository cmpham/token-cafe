// Bounded, independent C protobuf decoder before Rust allocates message fields.
#include <stdlib.h>
#include "pb_decode.h"
#include "pb_encode.h"
#include "companion.pb.h"
int tc_wire_validate(const unsigned char *bytes, size_t size) {
    if (!bytes || !size || size > 4096) return 0;
    token_cafe_v2_Envelope *message = calloc(1, sizeof(*message));
    if (!message) return 0;
    pb_istream_t stream = pb_istream_from_buffer(bytes, size);
    int ok = pb_decode(&stream, token_cafe_v2_Envelope_fields, message);
    free(message);
    return ok;
}
int tc_wire_roundtrip(const unsigned char *bytes, size_t size, unsigned char *out, size_t *length) {
    if (!bytes || !size || size > 4096 || !out || !length || *length > 4096) return 0;
    token_cafe_v2_Envelope *message = calloc(1, sizeof(*message));
    if (!message) return 0;
    pb_istream_t input = pb_istream_from_buffer(bytes, size);
    pb_ostream_t output = pb_ostream_from_buffer(out, *length);
    int ok = pb_decode(&input, token_cafe_v2_Envelope_fields, message)
          && pb_encode(&output, token_cafe_v2_Envelope_fields, message);
    *length = output.bytes_written;
    free(message);
    return ok;
}
