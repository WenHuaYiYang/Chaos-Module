#include <assert.h>
#include <string.h>
#include "../syspatch_blur/chaos_filter.h"
int main(void) {
    uint16_t input[84*120],old[84*120],v2[84*120],reference[84*120],work[84*120];
    for(unsigned i=0;i<84*120;i++) input[i]=(uint16_t)(i*37u);
    memcpy(old,input,sizeof(input));memcpy(reference,input,sizeof(input));
    chaos_v2_algorithm=0;
    assert(chaos_blur_filter(old,work,84,120,2,2,70));
    assert(rh_blur_filter_rgb565(reference,work,84,120,2,2,70));
    assert(!memcmp(old,reference,sizeof(old))); /* Corona remains bit exact. */
    memcpy(v2,input,sizeof(input));memcpy(reference,input,sizeof(input));
    syspatch_blur_buf_t b={reference,84,120,168,2};syspatch_blur_cfg_t cfg;
    syspatch_blur_cfg_default(&cfg);cfg.radius=12;assert(syspatch_blur_run(&b,&cfg));
    chaos_v2_algorithm=1;
    assert(chaos_blur_filter(v2,work,84,120,2,2,100));
    assert(!memcmp(v2,reference,sizeof(v2))); /* Original SysPatch filter. */
    assert(memcmp(v2,old,sizeof(v2))); /* V2 is not an alias of Corona. */
    for(unsigned i=0;i<84*120;i++) v2[i]=0xffff;
    assert(chaos_blur_filter(v2,work,84,120,2,2,70));
    for(unsigned i=0;i<84*120;i++) assert(v2[i]!=0); /* No black accumulator regression. */
    assert(!chaos_blur_filter(0,work,84,120,2,2,70));
    assert(!chaos_blur_filter(v2,work,85,120,2,2,70));
    return 0;
}
