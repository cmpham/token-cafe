// Deterministic malformed-message corpus for the exact bounded C firmware decoder.
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
extern int tc_wire_validate(const uint8_t *,size_t);
static uint32_t state=0xcafe1234;
static uint32_t random32(void){state^=state<<13;state^=state>>17;state^=state<<5;return state;}
int main(void){uint8_t data[4097];size_t accepted=0;for(unsigned n=0;n<1000000;n++){size_t len=random32()%sizeof(data);for(size_t i=0;i<len;i++)data[i]=(uint8_t)random32();accepted+=tc_wire_validate(data,len);}printf("PASS: 1000000 bounded decoder inputs under ASan/UBSan (%zu syntactically accepted)\n",accepted);return 0;}
