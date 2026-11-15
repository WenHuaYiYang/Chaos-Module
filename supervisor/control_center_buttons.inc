/* SPDX-License-Identifier: AGPL-3.0-only
 * p67 / .043 native button glass. The official widget, children, input,
 * checked/pressed state, coordinates and styles are never replaced or changed.
 * Each owned texture is written only after a GPU fence, never from a draw or
 * DELETE callback. Destruction of widgets merely retires the texture. */
#include "control_center_button_pixels.h"
#define CC_BUTTON_LIMIT 16u
#define CC_BUTTON_CLASS 0x2cdb9d6cu
struct cc_button { uint32_t object,root,generation,image,stamp; unsigned ready,w,h,radius; int x,y; };
static struct cc_button cc_buttons[CC_BUTTON_LIMIT];
static uint32_t cc_serial,cc_draws,cc_updates,cc_failures;
static unsigned cc_working;
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
static int cc_fence(void) { return p67_blur_native_fence() && cc_idle(); }
static int cc_moving(void) { return p67_surfaces[0].moving; }
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
       *stride<w*4 || *stride>1024 || *size<*stride*h || *size>131072 ||
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
    p67_blur_native_clean();
}
static int cc_source(struct cc_pixels *pixels,uint32_t *stamp) {
    struct p67_blur_pixel_view view;
    if(!p67_blur_source || !p67_blur_native_descriptor((uint32_t)p67_blur_source,15,&view)) return 0;
    pixels->data=view.pixels;pixels->width=view.width;pixels->height=view.height;
    pixels->stride=(unsigned)view.stride;pixels->size=(unsigned)view.size;
    *stamp=p67_blur_publishes;
    return 1;
}
static void cc_submit(uint32_t layer,uint32_t image,const int *area) {
    uint32_t descriptor[28];
    ((void (*)(void *))(uintptr_t)P67_NATIVE(0x0c5882b9u))(descriptor);
    descriptor[7]=image;
    /* Preserve a little of the official blue/pressed fill below the glass. */
    ((unsigned char *)descriptor)[76]=216u;
    ((void (*)(uint32_t,const void *,const int *))(uintptr_t)
        P67_NATIVE(0x0c5882a1u))(layer,descriptor,area);
}
#endif

static int cc_wanted(void) { return chaos_cc_buttons_demand(); }
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
        if(code==36) { b->object=0;b->ready=0;return; }
        if(cc_working || !cc_wanted() || !b->ready || !b->image || b->root!=cc_root() ||
           !cc_attached(object,b->root) || !words[4]) return;
        int area[4];cc_coords(object,area);
        if(area[2]-area[0]+1!=(int)b->w || area[3]-area[1]+1!=(int)b->h) return;
        cc_submit(words[4],b->image,area);++cc_draws;return;
    }
}
struct cc_scan { uint32_t objects[CC_BUTTON_LIMIT]; unsigned count,visited,overflow; };
static int cc_find(uint32_t object,void *cookie) {
    struct cc_scan *s=cookie;
    if(++s->visited>256) { s->overflow=1;return 2; }
    if(cc_class(object)!=CC_BUTTON_CLASS) return 0;
    if(s->count==CC_BUTTON_LIMIT) { s->overflow=1;return 2; }
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
    uint32_t *record=cc_record(object);
    if(record && record[1]==generation && record[2]==0) cc_remove(object);
    if(cc_live(object)) cc_invalidate(object);
}
static void cc_buttons_poll(void) {
    if(cc_working || !cc_idle()) return;
    cc_working=1;
    uint32_t root=cc_wanted()?cc_root():0;
    struct cc_scan scan={0};
    if(root) cc_walk(root,cc_find,&scan);
    if(scan.overflow || root!=(cc_wanted()?cc_root():0)) { ++cc_failures;goto done; }
    for(unsigned i=0;i<CC_BUTTON_LIMIT;++i) {
        struct cc_button *b=&cc_buttons[i];
        if(b->object && (b->root!=root || !cc_seen(&scan,b->object) || !cc_live(b->object))) cc_detach(b);
        if(b->object) {
            uint32_t *record=cc_record(b->object);
            if(!record || record[1]!=b->generation || record[2]!=0) cc_detach(b);
        }
    }
    for(unsigned i=0;i<scan.count;++i) {
        uint32_t object=scan.objects[i];unsigned j;
        if(!cc_attached(object,root)) continue;
        for(j=0;j<CC_BUTTON_LIMIT;++j) if(cc_buttons[j].object==object) break;
        if(j<CC_BUTTON_LIMIT) continue;
        for(j=0;j<CC_BUTTON_LIMIT;++j) if(!cc_buttons[j].object && !cc_buttons[j].image) break;
        if(j==CC_BUTTON_LIMIT || cc_record(object)) continue;
        struct cc_button *b=&cc_buttons[j];
        b->root=root;b->generation=++cc_serial;b->object=object;
        uint32_t generation=b->generation;
        uint32_t *added=cc_add(object,generation);
        if(b->object!=object || b->generation!=generation) continue;
        uint32_t *record=cc_live(object)?cc_record(object):0;
        if(!added || !record || record[1]!=generation || record[2]!=0) cc_detach(b);
    }
    /* One fence per changed batch, not one fence or snapshot per button/frame. */
    struct cc_pixels source;uint32_t stamp=0;
    int have_source=root && cc_source(&source,&stamp),fenced=0;
    for(unsigned i=0;i<CC_BUTTON_LIMIT;++i) {
        struct cc_button *b=&cc_buttons[i];
        if(!b->object) {
            if(b->image) {
                if(!fenced) { if(!cc_fence()) break;fenced=1; }
                uint32_t old=b->image;b->image=0;b->ready=0;cc_free(old);
            }
            continue;
        }
        if(!have_source || !cc_attached(b->object,root)) continue;
        int area[4];cc_coords(b->object,area);
        int w=area[2]-area[0]+1,h=area[3]-area[1]+1;
        if(w<1 || h<1 || w>160 || h>128 || area[0]>335 || area[1]>479 || area[2]<0 || area[3]<0) continue;
        unsigned radius=cc_radius(b->object);
        if(b->ready && b->stamp==stamp && b->x==area[0] && b->y==area[1] &&
           b->w==(unsigned)w && b->h==(unsigned)h && b->radius==radius) continue;
        /* Freeze existing local pixels during native scroll; refresh their
         * wallpaper crop when the page settles, without hiding the button. */
        if(b->ready && cc_moving()) continue;
        uint32_t object=b->object,generation=b->generation,current_stamp;
        if(!fenced) { if(!cc_fence()) break;fenced=1; }
        if(!cc_wanted() || cc_root()!=root || !cc_source(&source,&current_stamp) || current_stamp!=stamp) break;
        if(b->object!=object || b->generation!=generation || !cc_attached(object,root)) continue;
        if(b->image && (b->w!=(unsigned)w || b->h!=(unsigned)h)) {
            uint32_t old=b->image;b->image=0;b->ready=0;cc_free(old);
            if(b->object!=object || b->generation!=generation) continue;
        }
        if(!b->image) b->image=cc_alloc((unsigned)w,(unsigned)h);
        if(b->object!=object || b->generation!=generation || !b->image) { ++cc_failures;continue; }
        unsigned char *out;unsigned stride,size;
        if(!cc_buffer(b->image,(unsigned)w,(unsigned)h,&out,&stride,&size) ||
           !cc_glass_pixels(&source,out,(unsigned)w,(unsigned)h,stride,size,area[0],area[1],radius)) {
            b->ready=0;++cc_failures;continue;
        }
        cc_sync(b->image);
        if(b->object!=object || b->generation!=generation || !cc_attached(object,root)) continue;
        b->w=(unsigned)w;b->h=(unsigned)h;b->x=area[0];b->y=area[1];
        b->radius=radius;b->stamp=stamp;b->ready=1;++cc_updates;
        cc_invalidate(object);
    }
done:
    cc_working=0;
}
