#include "external_io.h"
#include <assert.h>
int main(void){
    ExternalButton b=external_button_init(true,0);
    assert(!external_button_sample(&b,true,1000)); // held at boot never approves
    assert(!external_button_sample(&b,false,1001));
    assert(!external_button_sample(&b,false,1031));assert(b.ready);
    assert(!external_button_sample(&b,true,1040));
    assert(!external_button_sample(&b,false,1050)); // bounce
    assert(!external_button_sample(&b,true,1060));
    assert(!external_button_sample(&b,true,1089));
    assert(external_button_sample(&b,true,1090));assert(b.stable);
    assert(!external_button_sample(&b,true,9000)); // no held-repeat
    assert(!external_button_sample(&b,false,9010));
    assert(external_button_sample(&b,false,9040));assert(!b.stable);
    assert(!external_button_sample(&b,false,9999));
    return 0;
}
