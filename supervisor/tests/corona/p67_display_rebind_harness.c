/* Reuse the pinned upstream lifecycle model without changing upstream files. */
#define main upstream_blur_main
#include "p67_blur_harness.c"
#undef main
#include "../src/corona_display_rebind.inc"

static void repair_poll(int demand) {
    unsigned c=copies,p=publishes,m=maintains,w=wakes;
    chaos_corona_display_rebind();
    poll(demand);
    assert(copies==c && publishes==p && maintains==m && wakes==w);
}
static void orphan(void) {
    assert(p67_blur_display && display_count);
    /* Unlike the upstream harness's full display deletion, the real event
     * dispatcher does not itself remove event records on code 36. */
    event(36);
    assert(!p67_blur_display && display_count && !p67_blur_source);
}
int main(void) {
    poll(1); event(52);
    assert(p67_blur_source && p67_blur_display_valid());
    orphan();
    for (unsigned i=0;i<3;++i) poll(1);
    assert(!p67_blur_display && !p67_blur_source && p67_blur_pending && p67_blur_armed);

    /* Unsafe render ownership, busy/reentrant polling, malformed/ambiguous
     * registrations must never authorize removal or duplicate subscriptions. */
    unsigned a=adds;
    owner_safe=0; repair_poll(1); assert(adds==a && display_count);
    owner_safe=1; p67_blur_busy=1; chaos_corona_display_rebind();
    assert(display_count); p67_blur_busy=0;
    p67_blur_polling=1; chaos_corona_display_rebind();
    assert(display_count); p67_blur_polling=0;
    uint32_t old=display_record[1];
    ++display_record[1]; repair_poll(1); assert(!p67_blur_display && adds==a);
    display_record[1]=old; display_record[2]=52;
    repair_poll(1); assert(!p67_blur_display && adds==a);
    display_record[2]=0x8000u; display_count=2;
    chaos_corona_display_rebind(); assert(display_count==2 && !p67_blur_display && adds==a);
    display_count=65; chaos_corona_display_rebind();
    assert(display_count==65 && !p67_blur_display && adds==a);
    display_count=1;

    uint32_t stale_event[5]={host_display,host_display,36,old,0};
    repair_poll(1);
    assert(chaos_corona_observer_repairs==1 && adds==a+1 && p67_blur_display_valid());
    p67_blur_display_event(stale_event); assert(p67_blur_display_valid());
    event(52);
    assert(p67_blur_source && !p67_blur_pending && p67_blur_core_gate==10);
    repair_poll(1); assert(chaos_corona_observer_repairs==1 && adds==a+1);

    /* Repeated app exits and disabled cleanup use the same safe repair. */
    orphan(); repair_poll(1); event(52);
    assert(p67_blur_source && chaos_corona_observer_repairs==2);
    orphan(); cleanup_fail=1; repair_poll(0); event(52);
    assert(P67_BLUR_CLEANUP_PENDING() && p67_blur_display_valid());
    cleanup_fail=0; repair_poll(0); event(52); repair_poll(0);
    assert(!P67_BLUR_CLEANUP_PENDING() && !p67_blur_display && !display_count);
    assert(chaos_corona_observer_repairs==3);
    /* A real deleted display with its list gone needs no orphan removal. */
    poll(1); event(52); orphan(); display_count=0;
    repair_poll(1); event(52);
    assert(p67_blur_source && p67_blur_display_valid() && chaos_corona_observer_repairs==3);
    puts("Chaos display orphan: baseline deadlock, guarded rebind, repeated capture, stale events and cleanup passed");
    return 0;
}
