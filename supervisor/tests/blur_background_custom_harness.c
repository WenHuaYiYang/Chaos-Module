/* Production background image task and timed refresh, with native task leaves. */
#include <stdint.h>
#include <stdio.h>
#include <assert.h>
#include <string.h>
#include "../blur_v2_params.h"
#include "../corona_vendor/include/resource_hook_background.h"
#define P67_BLUR_WIDTH 84u
#define P67_BLUR_HEIGHT 120u
#define CHAOS_BLUR_V2_COLOR 1
static uint32_t chaos_v2_params[BP_COUNT]=BP_DEFAULTS;
static unsigned chaos_corona_enabled=1,chaos_v2_algorithm=1,awake=1,p67_blur_pending;
static uint32_t now_ms,chaos_v2_background_last_capture,p67_blur_allowed_entries;
static unsigned p67_wake_pending,requests,draws,opacity;
static char p67_path[2][RH_PATH];static unsigned p67_path_index;
struct surface {char path[RH_PATH];};static struct surface p67_surfaces[3];
static const char *p67_surface_path(struct surface *s){return s->path;}
static uint32_t chaos_blur_screen_on(void){return awake;}
static uint32_t chaos_blur_monotonic_ms(void){return now_ms;}
static void p67_blur_request(void){++requests;p67_blur_pending=1;}
static uintptr_t p67_source(const char *path){return rh_background_is_blur(path)?77:0;}
static void p67_blur_draw_seen(void){}
static void initialize(void *d){memset(d,0,112);}
static void submit(uint32_t layer,const void *p,const int32_t *area){
    const uint32_t *d=p;assert(layer==99 && d[7]==77);
    assert(d[12]==256 && d[13]==256 && d[16]==0 && d[17]==0);
    assert(area[0]==0 && area[1]==0 && area[2]==335 && area[3]==479);
    assert(((const unsigned char *)d)[76]==opacity);++draws;
}
static uintptr_t native(uint32_t address){
    if(address==0x0c5882b9u)return (uintptr_t)initialize;
    assert(address==0x0c5882a1u);return (uintptr_t)submit;
}
#define P67_NATIVE(a) native(a)
#define P67_U32(a) ((a)==81u?(336u|(480u<<16)):(15u<<8))
#include CHAOS_BACKGROUND_DRAW
#include CHAOS_BACKGROUND_REFRESH
int main(void){
    int32_t area[4]={0,0,335,479};
    for(unsigned percent=0;percent<=100;percent+=50){
        chaos_v2_params[BP_BG_OPACITY]=percent;opacity=(percent*255+50)/100;
        p67_draw_background(99,RH_BG_BLUR,area);
    }
    assert(draws==3);
    strcpy(p67_path[0],RH_BG_BLUR);p67_blur_allowed_entries=1;
    now_ms=499;chaos_v2_background_refresh();assert(!requests);
    now_ms=500;chaos_v2_background_refresh();assert(requests==1 && p67_wake_pending);
    now_ms=2000;chaos_v2_background_refresh();assert(requests==1); // Pending capture not invalidated.
    p67_blur_pending=0;awake=0;chaos_v2_background_refresh();assert(requests==1);awake=1;
    p67_blur_allowed_entries=0;chaos_v2_background_refresh();assert(requests==1);
    p67_blur_allowed_entries=2;chaos_v2_background_refresh();assert(requests==1); // Unselected CC.
    strcpy(p67_surfaces[0].path,RH_BG_BLUR);
    chaos_v2_background_refresh();assert(requests==2);
    p67_blur_pending=0;chaos_corona_enabled=0;now_ms=10000;
    chaos_v2_background_refresh();assert(requests==2);chaos_corona_enabled=1;
    chaos_v2_params[BP_BG_INTERVAL]=3000;chaos_v2_background_last_capture=9000;
    chaos_v2_background_refresh();assert(requests==2);now_ms=12000;
    chaos_v2_background_refresh();assert(requests==3);
    p67_blur_pending=0;chaos_v2_background_last_capture=0xffffff00u;now_ms=0xe00u;
    chaos_v2_background_refresh();assert(requests==4);
    puts("PASS production background GPU opacity endpoints, independent interval, visibility, sleep/pending and wraparound gates");
}
