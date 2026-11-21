/* SPDX-License-Identifier: AGPL-3.0-only
 * p67 / .043 native button glass. The official widget, children, input,
 * checked/pressed state and coordinates are never replaced. The opaque native
 * fill is temporarily made translucent and restored on detach.
 * Each owned texture is written only after a GPU fence, never from a draw or
 * DELETE callback. Destruction of widgets merely retires the texture. */
#include "control_center_button_pixels.h"
#include "blur_v2_params.h"
#define CC_BUTTON_LIMIT 17u
#define CC_BUTTON_CLASS 0x2cdb9d6cu
struct cc_button { uint32_t object,root,generation,image,stamp; unsigned launcher,ready,w,h,radius,scale,original_opa,applied_opa,tinted; int x,y; };
static struct cc_button cc_buttons[CC_BUTTON_LIMIT];
static uint32_t cc_serial,cc_draws,cc_updates,cc_failures;
static unsigned cc_working;
static unsigned cc_scan_ticks;
static uint32_t cc_scan_root,cc_scan_launcher,cc_scan_launcher_root;
#ifndef CC_PARAM
static uint32_t cc_default_params[BP_COUNT]=BP_DEFAULTS;
#define CC_PARAM(i) cc_default_params[i]
#define CC_PARAM_EPOCH() 1u
#endif
static uint32_t cc_atlas[CC_ATLAS_W*CC_ATLAS_H] __attribute__((aligned(64)));
static uint32_t cc_atlas_source,cc_atlas_params,cc_atlas_serial,cc_frame_updates,cc_last_capture;
static unsigned cc_atlas_ready;
#ifndef CC_INTERACTION_BUSY
#define CC_INTERACTION_BUSY() 0
#endif
#ifndef CC_INTERACTION_NOTE
#define CC_INTERACTION_NOTE() ((void)0)
#endif
static int cc_observed_area[CC_BUTTON_LIMIT][4];
static uint32_t cc_observed_generation[CC_BUTTON_LIMIT];
static void cc_button_event(void *event);

#ifndef CHAOS_CC_BUTTONS_HOST
static int cc_idle(void) {
    return !p67_busy && !p67_blur_busy && !p67_blur_polling &&
           !p67_blur_native_capturing && p67_blur_native_idle();
}
static uint32_t cc_root(void) {
    uint32_t root=load32(0x2010fe50u);
    return p67_live(root) && load32(root)==0x2cdba8b4u?root:0;
}
static uint32_t cc_root_slot(void) { return load32(0x2010fe50u); }
static int cc_live(uint32_t object) { return p67_live(object); }
static uint32_t cc_class(uint32_t object) { return load32(object); }
static int cc_attached(uint32_t object,uint32_t root) {
    for(unsigned depth=0;object && depth<32;++depth) {
        /* Initial object was just observed by the native subtree walk/current
         * event. Native parent links, rather than retained child pointers. */
        if(((int (*)(uint32_t,uint32_t))(uintptr_t)P67_NATIVE(0x0c5881c1u))(object,1)) return 0;
        if(object==root) return 1;
        object=((uint32_t (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0c589579u))(object);
    }
    return 0;
}
static void cc_walk(uint32_t root,int (*callback)(uint32_t,void *),void *cookie) {
    ((void (*)(uint32_t,int (*)(uint32_t,void *),void *))(uintptr_t)
        P67_NATIVE(RH_FW_OBJECT_TREE_WALK))(root,callback,cookie);
}
static uint32_t *cc_record(uint32_t object) {
    return p67_descriptor(object,(uint32_t)(uintptr_t)cc_button_event);
}
static uint32_t *cc_add(uint32_t object,uint32_t generation) {
    return ((uint32_t *(*)(uint32_t,uint32_t,uint32_t,uint32_t))(uintptr_t)
        P67_NATIVE(0x0c5883f1u))(object,(uint32_t)(uintptr_t)cc_button_event,0,generation);
}
static void cc_remove(uint32_t object) {
    ((int (*)(uint32_t,uint32_t))(uintptr_t)P67_NATIVE(0x002bf811u))
        (object,(uint32_t)(uintptr_t)cc_button_event);
}
static void cc_invalidate(uint32_t object) {
    ((void (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0c10cbb5u))(object);
}
static void cc_coords(uint32_t object,int *area) {
    for(unsigned i=0;i<4;++i) area[i]=(int32_t)P67_U32(object+28u+i*4u);
}
static unsigned cc_radius(uint32_t object) {
    return ((uint32_t (*)(uint32_t,uint32_t,uint32_t))(uintptr_t)
        P67_NATIVE(0x0c1068a9u))(object,0,12);
}
static unsigned cc_opacity(uint32_t object) {
    return ((uint32_t (*)(uint32_t,uint32_t,uint32_t))(uintptr_t)
        P67_NATIVE(0x0c1068a9u))(object,0,29);
}
static void cc_set_opacity(uint32_t object,unsigned opacity) {
    ((void (*)(uint32_t,uint32_t,uint32_t))(uintptr_t)
        P67_NATIVE(0x0c5883f9u))(object,opacity,0);
}
static int cc_fence(void) { return p67_blur_native_fence() && cc_idle(); }
extern uint32_t chaos_blur_monotonic_ms(void),chaos_blur_screen_on(void);
static uint32_t cc_now(void) { return chaos_blur_monotonic_ms(); }
static unsigned cc_awake(void) { return chaos_blur_screen_on(); }
static void cc_request_source(void) { p67_blur_request();p67_wake_pending=1; }
static uint32_t cc_alloc(unsigned w,unsigned h) {
    return ((uint32_t (*)(uint32_t,uint32_t,uint32_t,uint32_t))(uintptr_t)
        P67_NATIVE(0x0c587f39u))(w,h,16,0);
}
static void cc_free(uint32_t image) {
    ((void (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0cac0db9u))(image);
}
static int cc_buffer(uint32_t image,unsigned w,unsigned h,unsigned char **out,unsigned *stride,unsigned *size) {
    if(!p67_blur_native_ram(image,28)) return 0;
    uint32_t header=P67_U32(image),pixels=P67_U32(image+16),allocation=P67_U32(image+20);
    *stride=P67_U32(image+8)&0xffffu; *size=P67_U32(image+12);
    if((header&0xffffu)!=0x1019u || P67_U32(image+4)!=(w|(h<<16)) ||
       *stride<w*4 || *stride>2048 || *size<*stride*h || *size>262144 ||
       (pixels&63u) || pixels<allocation || pixels-allocation>63 ||
       !p67_blur_native_ram(allocation,*size+pixels-allocation) ||
       P67_U32(image+24)!=0x20103244u) return 0;
    /* Native renderer accepts the same premultiplied ARGB8888 flag as S541. */
    P67_U32(image)=header|0x10000u;
    *out=(unsigned char *)(uintptr_t)pixels;
    return 1;
}
static void cc_sync(uint32_t image) {
    /* A reused descriptor may have cached header/decoded metadata. */
    ((void (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0c5890d1u))(image);
}
static void cc_flush(void) { p67_blur_native_clean(); }
static int cc_source(struct cc_pixels *pixels,uint32_t *stamp) {
    if(!p67_blur_source) return 0;
    for(unsigned i=0;i<2;++i) {
        struct p67_blur_native_output *out=&p67_blur_native_outputs[i];
        if(out->descriptor!=p67_blur_source || out->retired) continue;
        *pixels=(struct cc_pixels){out->buttons,84,120,84*3,sizeof(out->buttons)};
        *stamp=p67_blur_publishes;
        return 1;
    }
    return 0;
}
static void cc_submit(uint32_t layer,uint32_t image,const int *area) {
    uint32_t descriptor[28];
    ((void (*)(void *))(uintptr_t)P67_NATIVE(0x0c5882b9u))(descriptor);
    descriptor[7]=image;
    unsigned w=(unsigned)(area[2]-area[0]+1),h=(unsigned)(area[3]-area[1]+1);
    unsigned scale=!(w&1u) && !(h&1u)?2u:1u;
    int source_area[4]={area[0],area[1],area[0]+(int)(w/scale)-1,area[1]+(int)(h/scale)-1};
    descriptor[12]=descriptor[13]=256u*scale;
    descriptor[16]=descriptor[17]=0;
    /* Stronger official blue/pressed-state contribution, while the bounded
     * dark glass above it keeps white icons readable on bright wallpaper. */
    ((unsigned char *)descriptor)[76]=(unsigned char)((CC_PARAM(3)*255u+50u)/100u);
    ((void (*)(uint32_t,const void *,const int *))(uintptr_t)
        P67_NATIVE(0x0c5882a1u))(layer,descriptor,source_area);
}
#endif

#ifndef CC_LAUNCHER_DEMAND
#ifdef CHAOS_CC_BUTTONS_HOST
#define CC_LAUNCHER_DEMAND() 0
#define CC_LAUNCHER_TARGET(root) (*(root)=0,0u)
#else
extern uint32_t chaos_blur_launcher_target(uint32_t *root);
#define CC_LAUNCHER_DEMAND() chaos_launcher_buttons_demand()
#define CC_LAUNCHER_TARGET(root) chaos_blur_launcher_target(root)
#endif
#endif
static int cc_wanted(void) { return chaos_cc_buttons_demand() || CC_LAUNCHER_DEMAND(); }
static int cc_current(const struct cc_button *b) {
    if(!b->launcher) return chaos_cc_buttons_demand() && b->root==cc_root_slot();
    uint32_t root=0;
    return CC_LAUNCHER_DEMAND() && CC_LAUNCHER_TARGET(&root)==b->object && root==b->root;
}
static unsigned cc_resources(void) {
    unsigned n=0;
    for(unsigned i=0;i<CC_BUTTON_LIMIT;++i) n+=!!(cc_buttons[i].object || cc_buttons[i].image);
    return n;
}
static unsigned cc_cleanup_pending(void) { return !cc_wanted() && cc_resources(); }
static void cc_button_event(void *event) {
    uint32_t *words=event,object=words[0],generation=words[3];
    unsigned code=words[2]&0x7fffu;
    if(object!=words[1] || (code!=25 && code!=36)) return;
    for(unsigned i=0;i<CC_BUTTON_LIMIT;++i) {
        struct cc_button *b=&cc_buttons[i];
        if(b->object!=object || b->generation!=generation) continue;
        if(code==36) { b->object=0;b->ready=0;b->tinted=0;cc_scan_ticks=0;return; }
        if(cc_working || !cc_wanted() || !b->ready || !b->image || !cc_current(b) ||
           !cc_attached(object,b->root) || !words[4]) return;
        int area[4];cc_coords(object,area);
        if(area[2]-area[0]+1!=(int)b->w || area[3]-area[1]+1!=(int)b->h) return;
        cc_submit(words[4],b->image,area);++cc_draws;return;
    }
}
struct cc_scan { uint32_t objects[CC_BUTTON_LIMIT],roots[CC_BUTTON_LIMIT]; unsigned count,visited,overflow; };
static int cc_find(uint32_t object,void *cookie) {
    struct cc_scan *s=cookie;
    if(++s->visited>256) { s->overflow=1;return 2; }
    if(cc_class(object)!=CC_BUTTON_CLASS) return 0;
    if(s->count==CC_BUTTON_LIMIT-1u) { s->overflow=1;return 2; }
    s->objects[s->count++]=object;return 0;
}
static int cc_seen(const struct cc_scan *scan,uint32_t object) {
    for(unsigned i=0;i<scan->count;++i) if(scan->objects[i]==object) return 1;
    return 0;
}
static void cc_detach(struct cc_button *b) {
    uint32_t object=b->object,generation=b->generation;
    b->ready=0;b->object=0;
    if(!object || !cc_live(object)) return;
    if(b->tinted && cc_opacity(object)==b->applied_opa) cc_set_opacity(object,b->original_opa);
    b->tinted=0;
    if(!cc_live(object)) return;
    uint32_t *record=cc_record(object);
    if(record && record[1]==generation && record[2]==0) cc_remove(object);
    if(cc_live(object)) cc_invalidate(object);
}
static void cc_update_pixels(int frame) {
    struct cc_pixels source;uint32_t stamp=0;
    /* Retirement must work even after demand or the source disappeared. */
    unsigned fenced=0;
    if(!frame) for(unsigned i=0;i<CC_BUTTON_LIMIT;++i) {
        struct cc_button *b=&cc_buttons[i];
        if(!b->object && b->image) {
            if(!fenced) {if(!cc_fence())return;fenced=1;}
            uint32_t old=b->image;b->image=0;b->ready=0;cc_free(old);
        }
    }
    if(!cc_wanted() || !cc_awake() || !cc_source(&source,&stamp)) return;
    int atlas_dirty=!cc_atlas_ready || cc_atlas_source!=stamp || cc_atlas_params!=CC_PARAM_EPOCH();
    uint32_t changed[CC_BUTTON_LIMIT]={0},generations[CC_BUTTON_LIMIT]={0};
    unsigned visible=0,flushed=0;
    unsigned source_changed=atlas_dirty;
    for(unsigned i=0;i<CC_BUTTON_LIMIT;++i) {
        struct cc_button *b=&cc_buttons[i];
        if(!b->object) continue;
        if(!cc_current(b) || !cc_attached(b->object,b->root)) continue;
        int area[4];cc_coords(b->object,area);
        int w=area[2]-area[0]+1,h=area[3]-area[1]+1;
        if(w<1 || h<1 || w>336 || h>128 || area[0]>335 || area[1]>479 || area[2]<0 || area[3]<0) continue;
        ++visible;
        if(frame && !b->ready) continue;
        unsigned radius=frame?b->radius:cc_radius(b->object);
        unsigned tint=(CC_PARAM(4)*255u+50u)/100u;
        int shape=!b->ready || b->w!=(unsigned)w || b->h!=(unsigned)h || b->radius!=radius;
        if(frame && shape) continue;
        uint32_t object=b->object,generation=b->generation,current_stamp;
        if(!frame && b->tinted && b->applied_opa!=tint) {
            if(cc_opacity(b->object)==b->applied_opa) cc_set_opacity(b->object,tint);
            if(b->object!=object || b->generation!=generation) continue;
            b->applied_opa=tint;
        }
        /* Half-resolution sprites do not change when both origins still
         * address the same atlas texel. Keep exact truncation at negative edges. */
        int same_position=b->x==area[0] && b->y==area[1];
        if(b->scale==2 && b->x>=0 && b->y>=0 && area[0]>=0 && area[1]>=0)
            same_position=b->x/2==area[0]/2 && b->y/2==area[1]/2;
        if(!shape && !atlas_dirty && b->stamp==cc_atlas_serial && (!frame || same_position)) continue;
        if(!fenced) {if(!cc_fence())break;fenced=1;}
        if(!cc_wanted() || !cc_current(b) || !cc_source(&source,&current_stamp) || current_stamp!=stamp) break;
        if(b->object!=object || b->generation!=generation || !cc_attached(object,b->root)) continue;
        if(atlas_dirty) {
            struct chaos_blur_color style={CC_PARAM(BP_BUTTON_BRIGHTNESS),CC_PARAM(BP_BUTTON_SATURATION),CC_PARAM(BP_BUTTON_CONTRAST),
                CC_PARAM(BP_BUTTON_TINT),CC_PARAM(BP_BUTTON_RED),CC_PARAM(BP_BUTTON_GREEN),CC_PARAM(BP_BUTTON_BLUE)};
            if(!cc_build_atlas_styled(&source,cc_atlas,&style)) {++cc_failures;break;}
            cc_atlas_source=stamp;cc_atlas_params=CC_PARAM_EPOCH();++cc_atlas_serial;
            cc_atlas_ready=1;atlas_dirty=0;
        }
        if(b->image && (b->w!=(unsigned)w || b->h!=(unsigned)h)) {
            uint32_t old=b->image;b->image=0;b->ready=0;cc_free(old);
            if(b->object!=object || b->generation!=generation) continue;
        }
        unsigned scale=!(w&1) && !(h&1)?2u:1u;
        unsigned tw=(unsigned)w/scale,th=(unsigned)h/scale;
        if(!b->image) {if(frame)continue;b->image=cc_alloc(tw,th);}
        if(b->object!=object || b->generation!=generation || !b->image) {++cc_failures;continue;}
        unsigned char *out;unsigned stride,size;
        if(!cc_buffer(b->image,tw,th,&out,&stride,&size) ||
           (shape && !cc_prepare_mask(out,tw,th,stride,size,radius/scale)) ||
           !cc_copy_atlas(cc_atlas,out,tw,th,stride,size,area[0],area[1],scale)) {
            b->ready=0;++cc_failures;continue;
        }
        if(shape) cc_sync(b->image);
        if(b->object!=object || b->generation!=generation) continue;
        b->w=(unsigned)w;b->h=(unsigned)h;b->x=area[0];b->y=area[1];
        b->radius=radius;b->scale=scale;b->stamp=cc_atlas_serial;b->ready=1;
        ++cc_updates;if(frame)++cc_frame_updates;changed[i]=object;generations[i]=generation;
        if(!frame && !b->tinted) {
            b->original_opa=cc_opacity(object);b->applied_opa=tint;b->tinted=1;
            cc_set_opacity(object,tint);
        }
    }
    for(unsigned i=0;i<CC_BUTTON_LIMIT;++i) if(changed[i]) {
        if(!flushed) {cc_flush();flushed=1;}
        if(cc_buttons[i].object==changed[i] && cc_buttons[i].generation==generations[i] &&
           cc_current(&cc_buttons[i]) && (!frame || source_changed)) cc_invalidate(changed[i]);
    }
    /* Source capture is bounded separately from sprite updates. Replenish the
     * existing safe START budget; never capture a framebuffer containing icons. */
    if(!frame && visible) {
        uint32_t now=cc_now();
        if(now-cc_last_capture>=CC_PARAM(5)) {cc_last_capture=now;cc_request_source();}
    }
}
static void cc_buttons_frame(void) {
    if(cc_working || !cc_wanted() || !cc_resources() || !cc_idle()) return;
    // Observe geometry separately from published texture coordinates. Comparing
    // only b->x/y would renew the gate forever while a stale texture is held.
    unsigned geometry_moved=0;
    for(unsigned i=0;i<CC_BUTTON_LIMIT;++i) {
        struct cc_button *b=&cc_buttons[i];
        if(!b->object || !b->ready || !cc_current(b) || !cc_attached(b->object,b->root)) continue;
        int area[4];cc_coords(b->object,area);
        unsigned moved=0;
        if(cc_observed_generation[i]==b->generation) {
            for(unsigned j=0;j<4;++j) moved|=cc_observed_area[i][j]!=area[j];
        } else moved=area[0]!=b->x || area[1]!=b->y ||
            area[2]-area[0]+1!=(int)b->w || area[3]-area[1]+1!=(int)b->h;
        for(unsigned j=0;j<4;++j) cc_observed_area[i][j]=area[j];
        cc_observed_generation[i]=b->generation;
        geometry_moved|=moved;
    }
    if(geometry_moved) CC_INTERACTION_NOTE();
    if(CC_INTERACTION_BUSY()) return;
    cc_working=1;cc_update_pixels(1);cc_working=0;
}
static void cc_buttons_poll(void) {
    if(!cc_wanted() && !cc_resources()) return;
    // DELETE still clears logical ownership immediately; off/cleanup bypasses
    // this gate and retains all original fence/retirement protections.
    if(cc_wanted() && CC_INTERACTION_BUSY()) return;
    /* Draw events use the native GPU image path and raw root identity. Bound
     * expensive integrity scans to one in eight owner polls when cached. */
    uint32_t root=chaos_cc_buttons_demand()?cc_root_slot():0;
    uint32_t launcher_root=0,launcher=CC_LAUNCHER_DEMAND()?CC_LAUNCHER_TARGET(&launcher_root):0;
    if(cc_wanted() && cc_scan_ticks && root==cc_scan_root &&
       launcher==cc_scan_launcher && launcher_root==cc_scan_launcher_root) {
        unsigned complete=1;
        for(unsigned i=0;i<CC_BUTTON_LIMIT;++i)
            if(cc_buttons[i].image && !cc_buttons[i].object) complete=0;
            else if(cc_buttons[i].object && (!cc_buttons[i].ready || !cc_current(&cc_buttons[i]))) complete=0;
        if(complete && ++cc_scan_ticks<=8u) {
            if(!cc_working && cc_idle()) {cc_working=1;cc_update_pixels(0);cc_working=0;}
            return;
        }
    }
    if(cc_working || !cc_idle()) return;
    cc_working=1;
    root=chaos_cc_buttons_demand()?cc_root():0;
    struct cc_scan scan={0};
    if(root) cc_walk(root,cc_find,&scan);
    for(unsigned i=0;i<scan.count;++i) scan.roots[i]=root;
    if(launcher && launcher_root && cc_attached(launcher,launcher_root) &&
       cc_class(launcher)==CC_BUTTON_CLASS && scan.count<CC_BUTTON_LIMIT) {
        scan.objects[scan.count]=launcher;scan.roots[scan.count++]=launcher_root;
    }
    if(scan.overflow || (root && root!=cc_root_slot())) { ++cc_failures;goto done; }
    cc_scan_ticks=(root || launcher)?1u:0u;
    cc_scan_root=root;cc_scan_launcher=launcher;cc_scan_launcher_root=launcher_root;
    for(unsigned i=0;i<CC_BUTTON_LIMIT;++i) {
        struct cc_button *b=&cc_buttons[i];
        if(b->object && (!cc_current(b) || !cc_seen(&scan,b->object))) cc_detach(b);
        if(b->object) {
            uint32_t *record=cc_record(b->object);
            if(!record || record[1]!=b->generation || record[2]!=0) cc_detach(b);
        }
    }
    for(unsigned i=0;i<scan.count;++i) {
        uint32_t object=scan.objects[i],object_root=scan.roots[i];unsigned j;
        if(!cc_attached(object,object_root)) continue;
        for(j=0;j<CC_BUTTON_LIMIT;++j) if(cc_buttons[j].object==object) break;
        if(j<CC_BUTTON_LIMIT) continue;
        for(j=0;j<CC_BUTTON_LIMIT;++j) if(!cc_buttons[j].object && !cc_buttons[j].image) break;
        if(j==CC_BUTTON_LIMIT || cc_record(object)) continue;
        struct cc_button *b=&cc_buttons[j];
        b->root=object_root;b->launcher=(object==launcher && object_root==launcher_root);b->generation=++cc_serial;b->object=object;
        uint32_t generation=b->generation;
        uint32_t *added=cc_add(object,generation);
        if(b->object!=object || b->generation!=generation) continue;
        uint32_t *record=cc_record(object);
        if(!added || !record || record[1]!=generation || record[2]!=0) cc_detach(b);
    }
    cc_update_pixels(0);

done:
    cc_working=0;
}
