/* Actual native publication and button-source selection with allocator seams. */
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "../syspatch_blur/chaos_filter.h"
#include "../corona_vendor/include/resource_hook_p67_blur.h"
#include "../control_center_button_pixels.h"
#define CHAOS_BLUR_V2_COLOR 1
#include CHAOS_OUTPUT_STRUCT
static struct p67_blur_native_output p67_blur_native_outputs[2];
static unsigned idle=1,bad_descriptor,cleans;
static uint32_t next_descriptor=1,p67_blur_native_error,p67_blur_native_output_allocs,p67_blur_publishes;
static uintptr_t p67_blur_source;
static unsigned char full[2][336*480*3];
static uint32_t create(uint32_t w,uint32_t h,uint32_t format,uint32_t stride){
    assert(w==336 && h==480 && format==15 && !stride);return next_descriptor++;
}
#define P67_NATIVE(address) ((assert((address)==0x0c587f39u)),(uintptr_t)create)
static int p67_blur_native_idle(void){return idle;}
static void p67_blur_native_maintain(void){}
static void p67_blur_native_clean(void){++cleans;}
static int p67_blur_native_descriptor_for(uint32_t d,unsigned format,unsigned w,unsigned h,struct p67_blur_pixel_view *v){
    assert(d>=1 && d<=2 && format==15 && w==336 && h==480);
    *v=(struct p67_blur_pixel_view){full[d-1],sizeof(full[0]),336*3,336,480,15};return !bad_descriptor;
}
static int p67_blur_native_output_descriptor(uint32_t d,struct p67_blur_pixel_view *v){(void)d;(void)v;assert(0);return 0;}
#include CHAOS_OUTPUT_PUBLISH
#include CHAOS_BUTTON_SOURCE
int main(void){
    uint16_t plane[P67_BLUR_PIXELS]={0};struct cc_pixels pixels;uint32_t stamp;
    chaos_v2_algorithm=1;chaos_v2_width=84;chaos_v2_height=120;
    memset(chaos_v2_color,70,sizeof(chaos_v2_color));
    memset(chaos_v2_button_color,111,sizeof(chaos_v2_button_color));
    assert(!cc_source(&pixels,&stamp));
    idle=0;assert(!p67_blur_native_publish(plane,1));idle=1;
    uintptr_t first=p67_blur_native_publish(plane,1);assert(first==1 && cleans==1);
    p67_blur_source=first;p67_blur_publishes=10;
    assert(cc_source(&pixels,&stamp) && stamp==10 && pixels.width==84 && pixels.height==120);
    for(unsigned i=0;i<pixels.size;++i)assert(pixels.data[i]==111);
    const unsigned char *old=pixels.data;
    memset(chaos_v2_color,30,sizeof(chaos_v2_color));
    memset(chaos_v2_button_color,222,sizeof(chaos_v2_button_color));
    uintptr_t second=p67_blur_native_publish(plane,2);assert(second==2 && cleans==2);
    // Uncommitted next output cannot change an older published button source.
    assert(cc_source(&pixels,&stamp) && pixels.data==old && stamp==10);
    for(unsigned i=0;i<pixels.size;++i)assert(pixels.data[i]==111);
    p67_blur_source=second;p67_blur_publishes=11;
    assert(cc_source(&pixels,&stamp) && pixels.data!=old && stamp==11);
    for(unsigned i=0;i<pixels.size;++i)assert(pixels.data[i]==222);
    assert(full[0][0]==70 && full[1][0]==30);
    assert(!p67_blur_native_publish(plane,3));
    p67_blur_native_outputs[1].retired=1;assert(!cc_source(&pixels,&stamp));
    p67_blur_source=999;assert(!cc_source(&pixels,&stamp));
    p67_blur_native_outputs[0].descriptor=0;next_descriptor=1;bad_descriptor=1;
    memset(chaos_v2_button_color,99,sizeof(chaos_v2_button_color));
    assert(!p67_blur_native_publish(plane,4));p67_blur_source=1;
    assert(!cc_source(&pixels,&stamp));assert(cleans==2);
    puts("PASS independent button source follows exact immutable output generation and rejects failed/retired outputs");
}
