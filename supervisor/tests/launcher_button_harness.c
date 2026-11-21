/* Run the unchanged CC lifecycle suite, then exercise the second native root. */
#include <stdint.h>
static unsigned launcher_wanted;
static uint32_t launcher_object,launcher_root_id=30;
static uint32_t launcher_target(uint32_t *root);
#define CC_LAUNCHER_DEMAND() launcher_wanted
#define CC_LAUNCHER_TARGET(root) launcher_target(root)
#define main cc_original_suite
#include "cc_buttons_harness.c"
#undef main
static uint32_t launcher_target(uint32_t *root) {
    *root=launcher_object?launcher_root_id:0;
    return launcher_object;
}
static struct cc_button *owned(uint32_t object) {
    for(unsigned i=0;i<CC_BUTTON_LIMIT;++i) if(cc_buttons[i].object==object) return &cc_buttons[i];
    return NULL;
}
int main(void) {
    cc_original_suite();
    nodes[4]=(struct node){15,30,CC_BUTTON_CLASS,{0},1,0,{58,360,277,439},40,255};
    nodes[5]=(struct node){16,30,CC_BUTTON_CLASS,{0},1,0,{58,270,277,349},40,255};
    launcher_object=15;launcher_wanted=1;cc_buttons_poll();
    assert(owned(15) && owned(15)->ready && owned(15)->launcher);
    assert(!owned(14) && !owned(16)); /* Only the official button, no subtree replacement. */
    struct cc_button *b=owned(15);
    assert(images[b->image].w==110 && images[b->image].h==40 && b->scale==2);
    unsigned d=draws;draw(15);assert(draws==d+1);
    unsigned a=allocs,f=fences;
    for(unsigned i=0;i<120;++i){draw(15);cc_buttons_frame();cc_buttons_poll();}
    assert(allocs==a && fences==f); /* A stationary launcher does no extra GPU/CPU work. */
    ++stamp;cc_buttons_frame();assert(allocs==a && fences==f+1);
    params[BP_BUTTON_STATE_OPACITY]=20;++params_epoch;cc_buttons_poll();
    assert(node(15)->opacity==51); /* Same material controls as CC, including V3. */
    wanted=1;cc_scan_ticks=0;cc_buttons_poll();assert(owned(14)&&owned(15));
    launcher_wanted=0;cc_buttons_poll();
    assert(!owned(15)&&owned(14)&&node(15)->opacity==255&&!node(15)->record[0]);
    launcher_wanted=1;cc_scan_ticks=0;cc_buttons_poll();assert(owned(15));
    uint32_t generation=owned(15)->generation;erase(15);
    node(15)->live=1;node(15)->opacity=255;cc_buttons_poll();cc_scan_ticks=0;cc_buttons_poll();
    assert(owned(15) && owned(15)->generation!=generation);
    uint32_t stale[5]={15,15,36,generation,0};cc_button_event(stale);assert(owned(15));
    launcher_object=0;cc_buttons_poll(); /* S541 takeover/layout rebuild resolves no target. */
    assert(!owned(15)&&node(15)->opacity==255&&owned(14));
    launcher_object=16;cc_buttons_poll();assert(owned(16)&&!owned(15));
    assert(node(15)->area[0]==58 && node(15)->area[1]==360); /* Native geometry untouched. */
    launcher_wanted=0;empty();
    puts("Launcher glass: exact target, 220x80 GPU sprite, shared atlas/params, independent regions, rebuild/S541 release and stale DELETE passed");
    return 0;
}
