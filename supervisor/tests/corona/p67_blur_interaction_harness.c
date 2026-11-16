#include "blur_interaction.h"
static struct chaos_blur_interaction motion;
static uint32_t now_ms;
#define P67_BLUR_INTERACTION_BUSY() chaos_blur_interaction_busy(&motion,now_ms)
#define main original_blur_regression_main
#include "p67_blur_harness.c"
#undef main
int main(void) {
    poll(1);poll(1);event(52);event(53);assert(p67_blur_source);
    uintptr_t source=p67_blur_source;
    unsigned copy_count=copies,publish_count=publishes,wake_count=wakes;
    chaos_blur_interaction_note(&motion,now_ms,9);
    p67_blur_request();
    for(unsigned i=0;i<120;++i) {
        now_ms+=16;chaos_blur_interaction_note(&motion,now_ms,12);
        poll(1);event(52);event(53);
        assert(copies==copy_count && publishes==publish_count && wakes==wake_count);
        assert(p67_blur_source==source && p67_blur_pending);
    }
    chaos_blur_interaction_note(&motion,now_ms,11);
    now_ms+=159;poll(1);event(52);assert(copies==copy_count);
    ++now_ms;poll(1);event(52);event(53);
    assert(copies==copy_count+1 && publishes==publish_count+1 && !p67_blur_pending);
    event(52);assert(copies==copy_count+1); // no catch-up burst
    chaos_blur_interaction_note(&motion,now_ms,9);
    unsigned before=maintains;poll(0);event(52);
    assert(!p67_blur_source && maintains>before); // disable still cleans during motion
    chaos_blur_interaction_note(&motion,UINT32_MAX-79u,12);
    assert(chaos_blur_interaction_busy(&motion,79u));
    assert(!chaos_blur_interaction_busy(&motion,80u)); // missing END and clock wrap
    puts("PASS blur interaction: 120 gesture frames without COPY/publish/wake, pending retained, settle once, cleanup and clock wrap");
}
