/* Compile the production GPU-task submission body with task API leaf seams. */
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <stdio.h>
static unsigned expected_scale,expected_w,expected_h,calls;
static void initialize(void *p) { memset(p,0,112); }
static void submit(uint32_t layer,const void *p,const int *area) {
    const uint32_t *d=p;
    assert(layer==99 && d[7]==77);
    assert(d[12]==expected_scale && d[13]==expected_scale && d[16]==0 && d[17]==0);
    assert(((const unsigned char *)p)[76]==192);
    assert(area[0]==-10 && area[1]==70);
    assert((unsigned)(area[2]-area[0]+1)==expected_w);
    assert((unsigned)(area[3]-area[1]+1)==expected_h);
    ++calls;
}
static uintptr_t native(uint32_t address) {
    if(address==0x0c5882b9u)return (uintptr_t)initialize;
    assert(address==0x0c5882a1u);return (uintptr_t)submit;
}
#define P67_NATIVE(address) native(address)
#include CHAOS_CC_SUBMIT_SOURCE
int main(void) {
    int area[4]={-10,70,145,197};
    expected_scale=512;expected_w=78;expected_h=64;
    cc_submit(99,77,area);
    area[2]=144;area[3]=196;
    expected_scale=256;expected_w=155;expected_h=127;
    cc_submit(99,77,area);
    assert(calls==2);
    puts("CC production image tasks: GPU 2x scale/opacity/pivot/origin and odd-size 1x fallback passed");
}
