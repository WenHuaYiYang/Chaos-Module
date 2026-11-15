/* Fifth V2 region must work even when all four native backgrounds are off. */
static unsigned extra_demand;
#define P67_BLUR_EXTRA_DEMAND() extra_demand
#define main upstream_blur_main
#include "p67_blur_harness.c"
#undef main
int main(void) {
    poll(0);assert(!p67_blur_demand && !p67_blur_display);
    extra_demand=1;poll(0);event(52);
    assert(p67_blur_demand && p67_blur_source && p67_blur_display_valid());
    extra_demand=0;poll(0);event(52);poll(0);
    assert(!p67_blur_demand && !p67_blur_source && !p67_blur_display && !P67_BLUR_CLEANUP_PENDING());
    puts("V2 button-only demand: capture with four backgrounds off and deferred cleanup passed");
}
