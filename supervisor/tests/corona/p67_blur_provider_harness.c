/* Actual provider through native seams; not hardware cache acceptance. */
#define _GNU_SOURCE
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define RH_P67_BLUR_DIAGNOSTICS 1
#include "resource_hook_p67_blur.h"
#include "resource_hook_background.h"
#define RH_FW_DISPLAY_SLOT 0x2010318cu
#define RH_FW_FR_DISPLAY_LIST 0x2010317cu
static unsigned char *memory;
static void *pixels_at(uint32_t address) {
    assert(address >= 0x3c280000u && address < 0x3c680000u);
    return memory + address - 0x3c280000u;
}
static uint32_t watchface=0x3c280100u, display=0x200d0000u, layer=0x200d1000u;
static unsigned rendering,tasks,cyclic,fence_fail,fail_after,capture_fail,root_switch;
static unsigned root_live=1,hidden=1,allocations,destroys,captures,clean_calls,invalidate_calls,fence_calls;
static unsigned power_depth, power_stays, power_relaxes;
static unsigned barriers,malformed_input,malformed_output,alive[3],output_stride;
static uint32_t event_records[8][3];
static unsigned event_count,root_count,wake_calls;
static uint32_t root_record[3];
static int p67_wake_pending;
static int wake_frame(void) { ++wake_calls; return 0; }
static char operations[512];
static unsigned operation_count;
static void op(char c) { assert(operation_count<sizeof(operations)); operations[operation_count++]=c; }
static uint32_t rd32(uint32_t address) {
    if(address==RH_FW_DISPLAY_SLOT) return display;
    if(address==RH_FW_FR_DISPLAY_LIST) return 792u;
    if(address==RH_FW_FR_DISPLAY_LIST+4u || address==RH_FW_FR_DISPLAY_LIST+8u) return display;
    if(address==display+792u || address==display+796u) return 0;
    if(address==0x20119778u) return watchface;
    if(address==0x20103250u) return 0x1c04ecf5u;
    if(address==0x20103254u) return 0x1c04ed01u;
    if(address==display+680u) return layer;
    if(address==layer+100u) return tasks;
    if(address==layer+108u) return cyclic?layer:0;
    /* Native transition-pool/cache reads are deliberately unsupported. */
    assert(address>=0x3c280000u && address+4u<=0x3c680000u);
    return *(uint32_t *)pixels_at(address);
}
static unsigned rd8(uint32_t address) {
    assert(address==display+58u); return rendering?2:0;
}
static uint32_t load32(uint32_t address) { return rd32(address); }
static int p67_live(uint32_t object) { return root_live && object && object==watchface; }
static int power_stay(uint32_t lock) { assert(lock==0x200eb538u); ++power_depth; ++power_stays; return 0; }
static int power_relax(uint32_t lock) { assert(lock==0x200eb538u && power_depth); --power_depth; ++power_relaxes; return 0; }
static int finish(void) {
    assert(power_depth); /* Stock snapshot/fence contract requires PM protection. */
    ++fence_calls; op('F');
    if(fail_after && fence_calls>=fail_after) fence_fail=1;
    return fence_fail?4:0;
}
static int clean(uint32_t id,uint32_t address,uint32_t length) {
    assert(id==1 && address==0 && length==0x10000); ++clean_calls; op('C'); return 0;
}
static int invalidate(uint32_t id,uint32_t address,uint32_t length) {
    assert(id==1 && address==0x3c2a0040u && length==1008u*480u);
    assert(operation_count>=2 && operations[operation_count-2]=='F' && operations[operation_count-1]=='C');
    ++invalidate_calls; op('I'); return 0;
}
static void describe(uint32_t d,unsigned format,uint32_t allocation) {
    uint32_t width=format==15?336u:P67_BLUR_WIDTH;
    uint32_t height=format==15?480u:P67_BLUR_HEIGHT;
    uint32_t *p=pixels_at(d),bytes=format==15?1008u:output_stride;
    p[0]=25u|(format<<8)|(0x30u<<16); p[1]=width|(height<<16); p[2]=bytes;
    p[3]=bytes*height; p[4]=allocation+64u; p[5]=allocation+1u; p[6]=0x20103244u;
}
static uint32_t create(uint32_t width,uint32_t height,uint32_t format,uint32_t stride) {
    unsigned slot;
    assert((format==15 || format==18) && !stride);
    if(format==15) { assert(width==336 && height==480); slot=0; assert(!alive[0]); }
    else {
        assert(width==P67_BLUR_WIDTH && height==P67_BLUR_HEIGHT);
        slot=!alive[1]?1:2; assert(!alive[slot]);
    }
    ++allocations; alive[slot]=1; op('A');
    uint32_t d=0x3c280800u+64u*slot;
    describe(d,format,slot?0x3c340000u+0x100000u*(slot-1):0x3c2a0000u);
    if(slot) {
        uint32_t *header=pixels_at(d);
        memset(pixels_at(header[4]),0x5a,header[3]+16u);
    }
    if((format==15 && malformed_input) || (format==18 && malformed_output)) {
        ((uint32_t *)pixels_at(d))[1]=0; malformed_input=malformed_output=0;
    }
    return d;
}
static void destroy(uint32_t d) {
    unsigned slot=(d-0x3c280800u)/64u;
    assert(d>=0x3c280800u && slot<3 && alive[slot]);
    alive[slot]=0; ++destroys; op('D');
}
static int capture(uint32_t root,uint32_t format,uint32_t d) {
    assert(root==watchface && hidden && format==15 && d==0x3c280800u && alive[0] && power_depth);
    ++captures; op('S');
    unsigned char *p=pixels_at(((uint32_t *)pixels_at(d))[4]);
    for(unsigned i=0;i<336u*480u;++i) { p[3*i]=0; p[3*i+1]=0; p[3*i+2]=255; }
    if(root_switch) { root_switch=0; watchface+=64; }
    return capture_fail?0:1;
}
static uint32_t *event_add(uint32_t list,uint32_t callback,uint32_t filter,uint32_t generation) {
    assert(list==display+736 && filter==0x8000 && event_count<8);
    uint32_t *d=event_records[event_count++]; d[0]=callback; d[1]=generation; d[2]=filter; return d;
}
static uint32_t events_count(uint32_t list) { assert(list==display+736); return event_count; }
static uint32_t *event_get(uint32_t list,uint32_t i) { assert(list==display+736); return i<event_count?event_records[i]:0; }
static int event_remove(uint32_t list,uint32_t i) {
    assert(list==display+736 && i<event_count);
    memmove(event_records[i],event_records[i+1],(--event_count-i)*sizeof(event_records[0])); return 1;
}
static uint32_t *root_add(uint32_t object,uint32_t callback,uint32_t filter,uint32_t generation) {
    assert(object==watchface && filter==36 && !root_count);
    root_count=1; root_record[0]=callback; root_record[1]=generation; root_record[2]=filter; return root_record;
}
static uint32_t root_events_count(uint32_t object) { assert(object==watchface); return root_count; }
static uint32_t *root_event_get(uint32_t object,uint32_t index) { assert(object==watchface); return root_count && !index?root_record:0; }
static int root_remove(uint32_t object,uint32_t callback) {
    assert(object==watchface && root_count && root_record[0]==callback); root_count=0; return 1;
}
static uint32_t *p67_descriptor(uint32_t object,uint32_t callback) {
    return object==watchface && root_count && root_record[0]==callback?root_record:0;
}
static uintptr_t native(uint32_t address) {
    switch(address) {
    case 0x0c1a78f1u:return (uintptr_t)power_stay;
    case 0x0c1a79b5u:return (uintptr_t)power_relax;
    case 0x0c5883f1u:return (uintptr_t)root_add;
    case 0x002bf7d5u:return (uintptr_t)root_events_count;
    case 0x002bf7e9u:return (uintptr_t)root_event_get;
    case 0x002bf811u:return (uintptr_t)root_remove;
    case 0x0c187bb1u:return (uintptr_t)finish;
    case 0x0c588f89u:return (uintptr_t)clean;
    case 0x0c588449u:return (uintptr_t)invalidate;
    case 0x0c587f39u:return (uintptr_t)create;
    case 0x0cac0db9u:return (uintptr_t)destroy;
    case 0x0cac0879u:return (uintptr_t)capture;
    case 0x1c07a3e9u:return (uintptr_t)event_add;
    case 0x1c07ab49u:return (uintptr_t)events_count;
    case 0x1c07a4b1u:return (uintptr_t)event_get;
    case 0x1c07a6f9u:return (uintptr_t)event_remove;
    default:assert(!"unexpected native call");return 0;
    }
}
#define P67_NATIVE(address) native(address)
#define P67_U32(address) rd32(address)
#define P67_U8(address) rd8(address)
#define P67_BLUR_TEST_BARRIER() (++barriers)
#define P67_BLUR_TEST_PIXELS(address) ((const unsigned char *)pixels_at(address))
#include "../src/p67_blur_native.inc"
#define P67_BLUR_OWNER_SAFE() p67_blur_native_idle()
#define P67_BLUR_WAKE() wake_frame()
#include "../src/p67_blur.inc"
static void reset(void) {
    watchface=0x3c280100u; root_live=hidden=1;
    rendering=tasks=cyclic=fence_fail=fail_after=capture_fail=root_switch=0;
    allocations=destroys=captures=clean_calls=invalidate_calls=fence_calls=barriers=0;
    operation_count=malformed_input=malformed_output=event_count=0;
    output_stride=P67_BLUR_WIDTH*2u;
    memset(alive,0,sizeof(alive)); memset(p67_blur_native_outputs,0,sizeof(p67_blur_native_outputs));
    p67_blur_native_input=p67_blur_native_capturing=p67_blur_native_power_depth=0;
    root_count=wake_calls=0; p67_wake_pending=0;
    p67_blur_source=p67_blur_retired=0; p67_blur_epoch=p67_blur_config_epoch=0;
    p67_blur_root=p67_blur_root_generation=p67_blur_root_serial=0;
    p67_blur_display=p67_blur_display_generation=p67_blur_display_serial=0;
    p67_blur_root_registered=p67_blur_busy=p67_blur_polling=0;
    p67_blur_demand=p67_blur_visible=p67_blur_pending=p67_blur_wake=0;
    p67_blur_request_serial=0;
    p67_blur_armed=p67_blur_request_wake=p67_blur_cleanup_armed=0;
    p67_blur_frame52=p67_blur_attempts=p67_blur_copies=p67_blur_publishes=p67_blur_wakes=p67_blur_draws=0;
    p67_blur_core_gate=0;
    p67_blur_entry_scoped=p67_blur_allowed_entries=p67_blur_begin_skips=0;
    power_depth=power_stays=power_relaxes=0;
    p67_blur_native_pm_stays=p67_blur_native_pm_relaxes=0;
}
static void frame(unsigned code) {
    uint32_t event[6]={display,display,code,p67_blur_display_generation,0,0};
    p67_blur_display_event(event);
}
static void integrated_lifecycle(void) {
    reset(); p67_blur_reset(); p67_blur_watch(1);
    assert(!p67_source(RH_BG_BLUR));
    assert(root_count && event_count && !captures); /* Registration is not capture. */
    frame(52); assert(captures==1 && p67_source(RH_BG_BLUR)==p67_blur_source && p67_blur_source && !p67_blur_pending);
    frame(53); assert(wake_calls==1 && !p67_blur_wake);
    frame(52); assert(captures==1); /* Static output is not recaptured every frame. */
    uintptr_t old=p67_blur_source;
    p67_blur_request(); fail_after=fence_calls+2;
    frame(52); assert(p67_blur_pending && p67_blur_source==old && p67_blur_native_input);
    unsigned allocated=allocations;
    frame(53); frame(52);
    assert(allocations==allocated && p67_blur_source==old); /* Fence failure is bounded. */
    fail_after=fence_fail=0;
    p67_blur_watch(1); /* Bounded retry is rearmed by the one-second owner poll. */
    frame(53); frame(52);
    assert(!p67_blur_pending && p67_blur_source && p67_blur_source!=old && !p67_blur_native_input);
    assert(allocations==allocated+2); /* Input and replacement output, no extra snapshot lease. */
    p67_blur_request(); fail_after=fence_calls+3; /* Maintenance of retired old output precedes capture. */
    frame(52);
    assert(p67_blur_native_input && p67_blur_pending);
    unsigned taken=captures;
    p67_blur_watch(0); assert(!p67_blur_source && event_count && !root_count);
    assert(p67_blur_native_input); /* Disabling cannot free GPU-owned input. */
    unsigned fences=fence_calls;
    for(unsigned i=0;i<1000;++i) { frame(52); frame(53); }
    assert(fence_calls<=fences+1 && captures==taken); /* Cleanup is bounded too. */
    fail_after=fence_fail=0; p67_blur_watch(0);
    assert(p67_blur_native_input); /* Poll performs no native GPU work. */
    frame(52); p67_blur_watch(0);
    assert(!p67_blur_native_input && !alive[0] && !alive[1] && !alive[2] && captures==taken && !event_count);
    assert(!power_depth && power_stays==power_relaxes);
    char diag[1024];
    assert(rh_platform_p67_blur_diagnostics(diag,sizeof(diag)) && strstr(diag,"native_error=") && strstr(diag,"pm_stays="));
}
static void integrated_entry_exit(void) {
    reset();p67_blur_reset();p67_blur_watch(1);frame(52);frame(53);
    assert(captures==1 && p67_blur_source); /* Existing explicit initialization. */
    p67_wake_pending=0; /* Model launcher's consumption of the owner-poll wake. */
    struct p67_blur_session session={0};
    p67_blur_session_event(&session,1u,1u,300u,9u,480,480,1u);
    frame(52);frame(53);assert(captures==2 && !p67_blur_pending);
    p67_wake_pending=0;
    uintptr_t source=p67_blur_source;
    unsigned taken=captures,allocated=allocations,fences=fence_calls;
    uint32_t requested=p67_blur_request_serial;
    p67_blur_session_event(&session,1u,1u,300u,9u,120,480,1u);
    p67_blur_session_event(&session,1u,1u,300u,11u,120,480,1u);
    assert(p67_blur_request_serial==requested && p67_blur_source==source);
    p67_blur_session_event(&session,1u,1u,300u,11u,0,480,1u);
    p67_blur_session_event(&session,1u,1u,300u,9u,0,480,1u); /* Exit. */
    assert(!p67_blur_capture_allowed() && !p67_blur_pending && p67_blur_source==source);
    for(unsigned i=0;i<3;++i) {
        p67_blur_watch(1);frame(52);frame(53);
        assert(!p67_blur_armed && !p67_wake_pending && p67_blur_source==source);
    }
    assert(captures==taken && allocations==allocated && fence_calls==fences);
    /* A dirty lifecycle after exit cannot revive screenshot work at home. */
    p67_blur_drop();p67_blur_watch(1);frame(53); /* One source-removal repaint is valid. */
    unsigned wakes=wake_calls;
    for(unsigned i=0;i<20;++i) { frame(52);frame(53); }
    assert(captures==taken && allocations==allocated && fence_calls==fences);
    assert(p67_blur_pending && !p67_blur_armed && wake_calls==wakes);
    p67_blur_session_event(&session,1u,1u,300u,12u,480,480,1u);
    p67_blur_session_event(&session,1u,1u,300u,9u,480,480,1u);
    frame(52);frame(53);assert(captures==taken+1 && p67_blur_source);
    source=p67_blur_source;
    p67_blur_session_event(&session,1u,1u,300u,11u,0,480,1u);
    p67_blur_session_event(&session,1u,1u,300u,9u,0,480,1u);
    p67_blur_session_event(&session,1u,1u,300u,12u,480,480,1u);
    p67_blur_session_event(&session,1u,1u,300u,9u,480,480,1u);
    fail_after=fence_calls+2u;frame(52);
    assert(p67_blur_pending && p67_blur_native_input && p67_blur_source==source);
    taken=captures;fences=fence_calls;
    p67_blur_session_event(&session,1u,1u,300u,9u,0,480,1u);
    p67_blur_watch(1);frame(52);frame(53);
    assert(captures==taken && fence_calls==fences && p67_blur_native_input);
    assert(p67_blur_source==source && !p67_blur_armed);
    /* Exit does not bypass retirement safety. System restore still performs
     * disabled cleanup even when no entry is allowed, after fence recovery. */
    p67_blur_watch(0);frame(52);assert(p67_blur_native_input);
    fail_after=fence_fail=0;p67_blur_watch(0);frame(52);p67_blur_watch(0);
    assert(!p67_blur_native_input && !alive[0] && !alive[1] && !alive[2]);
    assert(!event_count && !power_depth && !p67_blur_native_power_depth);
}
static void lowres_storage(void) {
    uint16_t input[P67_BLUR_PIXELS];
    unsigned char pixels[(P67_BLUR_WIDTH*2u+3u)*P67_BLUR_HEIGHT+2u];
    const size_t stride=P67_BLUR_WIDTH*2u+3u,size=stride*P67_BLUR_HEIGHT;
    for(unsigned i=0;i<P67_BLUR_PIXELS;++i) input[i]=(uint16_t)(i*31u);
    memset(pixels,0x5a,sizeof(pixels));
    assert(!p67_blur_store_rgb565(0,pixels+1u,stride,size));
    assert(!p67_blur_store_rgb565(input,0,stride,size));
    assert(!p67_blur_store_rgb565(input,pixels+1u,P67_BLUR_WIDTH*2u-1u,size));
    assert(!p67_blur_store_rgb565(input,pixels+1u,stride,size-1u));
    assert(!p67_blur_store_rgb565(input,pixels+1u,SIZE_MAX,size));
    for(unsigned i=0;i<sizeof(pixels);++i) assert(pixels[i]==0x5a);
    assert(!p67_blur_store_rgb565(input,input,168u,sizeof(input)));
    assert(!p67_blur_store_rgb565(input,input+1u,168u,sizeof(input)));
    assert(!p67_blur_store_rgb565(input+1u,input,168u,sizeof(input)));
    for(unsigned i=0;i<P67_BLUR_PIXELS;++i) assert(input[i]==(uint16_t)(i*31u));
    assert(p67_blur_store_rgb565(input,pixels+1u,stride,size));
    assert(pixels[0]==0x5a && pixels[sizeof(pixels)-1u]==0x5a);
    for(unsigned y=0;y<P67_BLUR_HEIGHT;++y) {
        const unsigned char *row=pixels+1u+y*stride;
        for(unsigned x=0;x<P67_BLUR_WIDTH;++x) {
            unsigned v=input[y*P67_BLUR_WIDTH+x];
            assert(row[x*2u]==(unsigned char)v && row[x*2u+1u]==(unsigned char)(v>>8));
        }
        for(unsigned x=P67_BLUR_WIDTH*2u;x<stride;++x) assert(row[x]==0x5a);
    }
}
int main(void) {
    lowres_storage();
    memory=calloc(1,0x400000u); assert(memory);
    uint16_t plane[P67_BLUR_PIXELS]; uint32_t identity=0;
    struct p67_blur_pixel_view view;
    reset();
    assert(P67_BLUR_AVAILABLE && p67_blur_native_copy(plane,&identity) && identity==watchface && hidden);
    for(unsigned i=0;i<P67_BLUR_PIXELS;++i) assert(plane[i]==0xf800);
    assert(captures==1 && allocations==1 && destroys==1 && !alive[0]);
    assert(clean_calls==2 && invalidate_calls==1 && barriers);
    assert(fence_calls==3 && power_stays==1 && power_relaxes==1);
    assert(!power_depth && !p67_blur_native_power_depth);
    /* A standalone fence still acquires its own native PM protection. */
    assert(p67_blur_native_fence() && power_stays==2 && power_relaxes==2);
    assert(fence_calls==4 && !power_depth && !p67_blur_native_power_depth);
    assert(strchr(operations,'S') && !p67_blur_native_input);
    assert(p67_blur_native_display_add(display,0x12341,9));
    assert(p67_blur_native_display_count(display)==1);
    assert(p67_blur_native_display_get(display,0)[1]==9);
    assert(p67_blur_native_display_remove(display,0x12341) && !event_count);
    assert(!p67_blur_native_descriptor(0x3c2713f0u,15,&view));
    uint32_t manual=0x3c280400u,*d=pixels_at(manual); describe(manual,15,0x3c2a0000u);
    d[0]^=1; assert(!p67_blur_native_descriptor(manual,15,&view)); d[0]^=1;
    d[2]=1007; assert(!p67_blur_native_descriptor(manual,15,&view)); d[2]=1008;
    d[3]--; assert(!p67_blur_native_descriptor(manual,15,&view)); d[3]++;
    d[4]++; assert(!p67_blur_native_descriptor(manual,15,&view)); d[4]--;
    uint32_t pixels=d[4]; d[4]=0xffff0000u;
    assert(!p67_blur_native_descriptor(manual,15,&view)); d[4]=pixels;
    rendering=1; assert(!p67_blur_native_copy(plane,&identity)); rendering=0;
    tasks=1; assert(!p67_blur_native_copy(plane,&identity)); tasks=0;
    cyclic=1; assert(!p67_blur_native_copy(plane,&identity)); cyclic=0;
    root_live=0; assert(!p67_blur_native_copy(plane,&identity)); root_live=1;
    /* A low-resolution output shape must never pass the input validator. */
    describe(manual,18,0x3c340000u);
    assert(p67_blur_native_output_descriptor(manual,&view));
    assert(view.width==P67_BLUR_WIDTH && view.height==P67_BLUR_HEIGHT && view.stride==168u);
    assert(!p67_blur_native_descriptor(manual,18,&view));
    d[1]=336u|(480u<<16); assert(!p67_blur_native_output_descriptor(manual,&view));
    describe(manual,18,0x3c340000u);
    d[2]=169u;d[3]=169u*P67_BLUR_HEIGHT;
    assert(!p67_blur_native_output_descriptor(manual,&view));
    describe(manual,18,0x3c340000u);d[3]--;
    assert(!p67_blur_native_output_descriptor(manual,&view));
    malformed_input=1; assert(!p67_blur_native_copy(plane,&identity) && !alive[0]);
    capture_fail=1; assert(!p67_blur_native_copy(plane,&identity) && !alive[0]); capture_fail=0;
    root_switch=1; assert(!p67_blur_native_copy(plane,&identity) && !alive[0]); reset();
    fail_after=2; assert(!p67_blur_native_copy(plane,&identity));
    assert(allocations==1 && !destroys && p67_blur_native_input && alive[0] && !invalidate_calls);
    assert(!p67_blur_native_copy(plane,&identity) && allocations==1);
    assert(!power_depth && !p67_blur_native_power_depth && power_stays==power_relaxes);
    fail_after=fence_fail=0;
    assert(p67_blur_native_copy(plane,&identity) && !p67_blur_native_input && destroys==2);
    output_stride=192u; /* GPU-aligned padded output, still only 84x120. */
    uintptr_t first=p67_blur_native_publish(plane,1);
    assert(first);
    uint32_t *output=pixels_at((uint32_t)first);
    assert(output[1]==(P67_BLUR_WIDTH|(P67_BLUR_HEIGHT<<16)) && output[2]==192u);
    const unsigned char *published=pixels_at(output[4]);
    for(unsigned y=0;y<P67_BLUR_HEIGHT;++y) {
        for(unsigned x=0;x<P67_BLUR_WIDTH;++x)
            assert(published[y*output_stride+x*2u]==0 && published[y*output_stride+x*2u+1u]==0xf8);
        for(unsigned x=P67_BLUR_WIDTH*2u;x<output_stride;++x)
            assert(published[y*output_stride+x]==0x5a);
    }
    assert(published[output[3]]==0x5a);
    for(unsigned i=0;i<P67_BLUR_PIXELS;++i) plane[i]=(uint16_t)(i*31u);
    uintptr_t second=p67_blur_native_publish(plane,1);
    assert(second && first!=second);
    assert(*(const uint16_t *)published==0xf800); /* Scratch writes cannot mutate the live GPU source. */
    uint32_t *replacement=pixels_at((uint32_t)second);
    const unsigned char *pixels2=pixels_at(replacement[4]);
    for(unsigned y=0;y<P67_BLUR_HEIGHT;++y) for(unsigned x=0;x<P67_BLUR_WIDTH;++x) {
        unsigned v=plane[y*P67_BLUR_WIDTH+x];
        assert(pixels2[y*output_stride+x*2u]==(unsigned char)v);
        assert(pixels2[y*output_stride+x*2u+1u]==(unsigned char)(v>>8));
        assert(published[y*output_stride+x*2u+1u]==0xf8);
    }
    unsigned allocated=allocations,freed=destroys;
    assert(!p67_blur_native_publish(plane,1) && allocations==allocated);
    p67_blur_native_retire(first); p67_blur_native_retire(second);
    assert(destroys==freed); /* RETIRE does not fence or destroy. */
    rendering=1; p67_blur_native_maintain(); assert(destroys==freed); rendering=0;
    tasks=1; p67_blur_native_maintain(); assert(destroys==freed);
    assert(!p67_blur_native_publish(plane,2) && allocations==allocated);
    /* Model delayed consumption of the previously submitted image source. Its
     * copied descriptor still resolves to live, immutable pixels after retire;
     * CPU scratch/output replacement must not free or overwrite that storage. */
    assert(alive[1] && alive[2] && p67_blur_native_output_descriptor((uint32_t)first,&view));
    for(unsigned y=0;y<P67_BLUR_HEIGHT;++y) for(unsigned x=0;x<P67_BLUR_WIDTH;++x) {
        const unsigned char *queued=view.pixels+y*view.stride+x*2u;
        assert(queued[0]==0 && queued[1]==0xf8);
    }
    tasks=0;
    fence_fail=1; p67_blur_native_maintain(); assert(destroys==freed);
    assert(!p67_blur_native_publish(plane,1) && allocations==allocated); fence_fail=0;
    p67_blur_native_maintain(); assert(destroys==freed+2);
    malformed_output=1; assert(!p67_blur_native_publish(plane,1));
    p67_blur_native_maintain(); assert(destroys==freed+3);
    assert(p67_blur_native_publish(plane,1));
    integrated_lifecycle();
    integrated_entry_exit();
    free(memory);
    puts("p67 active blur provider: OK (host seams, not device cache acceptance)");
    return 0;
}
