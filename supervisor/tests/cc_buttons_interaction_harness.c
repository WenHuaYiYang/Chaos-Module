#include "../blur_interaction.h"
static struct chaos_blur_interaction motion;
#define CC_INTERACTION_BUSY() chaos_blur_interaction_busy(&motion,now_ms)
#define CC_INTERACTION_NOTE() chaos_blur_interaction_note(&motion,now_ms,12)
#define main original_button_regression_main
#include "cc_buttons_harness.c"
#undef main
int main(void) {
    memset(source_pixels,100,sizeof(source_pixels));
    nodes[0]=(struct node){11,10,CC_BUTTON_CLASS,{0},1,0,{0,20,155,147},36,255};
    nodes[1]=(struct node){12,10,CC_BUTTON_CLASS,{0},1,0,{88,200,247,265},33,255};
    wanted=1;cc_buttons_poll();cc_buttons_frame();
    unsigned f=fences,u=cc_updates,fl=flushes,scans=walks;
    assert(u==2);
    for(unsigned i=1;i<=120;++i) {
        now_ms+=16;node(11)->area[1]=20+(int)i;node(11)->area[3]=147+(int)i;
        cc_buttons_frame();cc_buttons_poll();draw(11);
        assert(fences==f && cc_updates==u && flushes==fl && walks==scans);
    }
    assert(draws==120 && cc_buttons[0].y==20); // old texture follows native geometry
    ++stamp;now_ms+=159;cc_buttons_frame();assert(fences==f);
    ++now_ms;cc_buttons_frame();
    assert(fences==f+1 && cc_updates==u+2 && cc_buttons[0].y==140);
    cc_buttons_frame();assert(fences==f+1); // stale published x/y cannot keep gate alive
    chaos_blur_interaction_note(&motion,now_ms,9);
    erase(11);empty(); // DELETE and disabled cleanup keep original safety gates
    puts("PASS button interaction: 120 moving frames draw cached sprites without fences/copies/scans, settle update once and safe disable");
}
