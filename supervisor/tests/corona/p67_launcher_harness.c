#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "resource_hook_background.h"
#define RH_TARGET_1043 1
#define RH_RELOAD_LIMIT 1024u
#define RH_FW_OBJECT_TREE_WALK 1u
#define RH_FW_IMAGE_GET_INFO 2u
#define RH_FW_HEADER_CACHE_SLOT 3u
#define RH_FW_IMAGE_CACHE_SLOT 4u
#define RH_FW_HEADER_CACHE_CLASS 5u
#define RH_FW_IMAGE_CACHE_CLASS 6u
#define OWNER 100u
#define PAGE 200u
#define FOREGROUND 300u
#define LIST 400u
#define NEW_LIST 500u
static uintptr_t native(uint32_t address);
static unsigned char *byte_at(uint32_t address);
static uint32_t *word_at(uint32_t address);
#define P67_NATIVE(address) native(address)
#define P67_U8(address) (*byte_at(address))
#define P67_U32(address) (*word_at(address))
struct object { uint32_t id, parent, clz, y, bounds[4], count, d[8][3]; int live; };
static struct object objects[5];
static unsigned char moving;
static uint32_t root_slot, image_slot, foreground_slot;
static unsigned add_calls, fail_add, mutate_binding, forwards[40], preparations, draws;
static unsigned tree_calls,forbid_page_coords;
static uint32_t layer_clip[4];
static int32_t expected_clip[4];
static uint32_t last_prepared;
static unsigned image_inits, image_draws;
static uint32_t last_image_descriptor[28];
static struct object *object(uint32_t id) {
    for (unsigned i = 0; i < 5; ++i) if (objects[i].id == id) return &objects[i];
    assert(!"unknown object"); return 0;
}
static unsigned char *byte_at(uint32_t a) { assert(a == 0x201102d9u); return &moving; }
static uint32_t *word_at(uint32_t a) {
    if(a>=PAGE+28u && a<=PAGE+40u) assert(!forbid_page_coords);
    if (a >= 77u + 24u && a <= 77u + 36u) return &layer_clip[(a - 77u - 24u) / 4u];
    for (unsigned i = 0; i < 5; ++i) {
        struct object *s = &objects[i];
        if (a >= s->id + 28u && a <= s->id + 40u) {
            unsigned index = (a - s->id - 28u) / 4u;
            return index == 1 ? &s->y : &s->bounds[index];
        }
    }
    assert(!"unknown word"); return 0;
}
static uint32_t load32(uint32_t a) {
    if (a == 0x201102ecu) return root_slot;
    if (a == 0x201102dcu) return image_slot;
    if (a == 0x201102f4u) return foreground_slot;
    if (a == RH_FW_HEADER_CACHE_SLOT || a == RH_FW_IMAGE_CACHE_SLOT) return 0;
    return object(a)->clz;
}




static unsigned redraw_requests;
static int redraw_result;
static int rh_platform_request_full_redraw(void) { ++redraw_requests; return redraw_result; }
#include "../src/p67_launcher.inc"
static void tree(uint32_t root, int (*cb)(uint32_t, void *), void *cookie) {
    assert(root == 0);++tree_calls;
    for (unsigned i = 0; i < 5; ++i) if (objects[i].live && cb(objects[i].id, cookie) == 2) break;
}
static uint32_t count(uint32_t o) { return object(o)->count; }
static uint32_t *get(uint32_t o, uint32_t index) {
    struct object *s = object(o); assert(s->live && index < s->count); return s->d[index];
}
static uint32_t parent(uint32_t o) { assert(object(o)->live); return object(o)->parent; }
static uint32_t *add(uint32_t o, uint32_t cb, uint32_t filter, uint32_t user) {
    struct object *s = object(o); assert(s->live);
    if (++add_calls == fail_add) return 0;
    if (mutate_binding && add_calls == 2) object(OWNER)->d[0][1] = FOREGROUND;
    assert(s->count < 8); uint32_t *d = s->d[s->count++];
    d[0] = cb; d[1] = user; d[2] = filter; return d;
}
static int remove_event(uint32_t o, uint32_t cb) {
    struct object *s = object(o); assert(s->live);
    for (unsigned i = 0; i < s->count; ++i) if (s->d[i][0] == cb) {
        memmove(&s->d[i], &s->d[i + 1], (s->count - i - 1) * sizeof(s->d[0]));
        --s->count; return 1;
    }
    return 0;
}
static void original(void *e) {
    unsigned code = *(uint16_t *)((unsigned char *)e + 8) & 0x7fffu; assert(code < 40);
    ++forwards[code]; if (code == 11) moving = 0;
}
static void prepare(uint32_t list, int a, int b) {
    assert(!a && !b); ++preparations; last_prepared = list;
}
static void rect_init(void *d) { memset(d, 0, 188); }
static void rect_draw(uint32_t layer, const void *d, const int32_t *area) {
    assert(layer == 77 && ((const uint32_t *)d)[30]);
    assert(!area[0] && !area[1] && area[2] == 335 && area[3] == 479);
    for (unsigned i = 0; i < 4; ++i) assert((int32_t)layer_clip[i] == expected_clip[i]);
    ++draws;
}
static void image_init(void *d) {
    memset(d,0,112); ((uint32_t *)d)[5]=112;
    ((uint32_t *)d)[12]=((uint32_t *)d)[13]=256;
    ((unsigned char *)d)[76]=255; ((unsigned char *)d)[77]=0x10;
    ++image_inits;
}
static void image_draw(uint32_t layer,const void *d,const int32_t *area) {
    const uint32_t *descriptor=d;
    assert(layer==77 && descriptor[5]==112 && descriptor[7]==p67_blur_source);
    assert(descriptor[12]==1024 && descriptor[13]==1024);
    assert(!descriptor[11] && !descriptor[16] && !descriptor[17]);
    assert(((const unsigned char *)d)[76]==255 && ((const unsigned char *)d)[77]==0x10);
    assert(!area[0] && !area[1] && area[2]==83 && area[3]==119);
    assert((area[2]+1)*(int32_t)descriptor[12]/256==336);
    assert((area[3]+1)*(int32_t)descriptor[13]/256==480);
    for(unsigned i=0;i<4;++i) assert((int32_t)layer_clip[i]==expected_clip[i]);
    memcpy(last_image_descriptor,d,sizeof(last_image_descriptor));
    ++image_draws; ++draws;
}
static int image_info(const char *p, void *out) { (void)out; assert(p[0]); return 1; }
static uintptr_t native(uint32_t a) {
    switch (a) {
    case RH_FW_OBJECT_TREE_WALK: return (uintptr_t)tree;
    case RH_FW_IMAGE_GET_INFO: return (uintptr_t)image_info;
    case 0x002bf7d5u: return (uintptr_t)count;
    case 0x002bf7e9u: return (uintptr_t)get;
    case 0x002bf811u: return (uintptr_t)remove_event;
    case 0x0c589579u: return (uintptr_t)parent;
    case 0x0c5883f1u: return (uintptr_t)add;
    case P67_SCROLL_CB: return (uintptr_t)original;
    case 0x0c5166f1u: return (uintptr_t)prepare;
    case 0x0c588269u: return (uintptr_t)rect_init;
    case 0x0c588351u: return (uintptr_t)rect_draw;
    case 0x0c5882b9u: return (uintptr_t)image_init;
    case 0x0c5882a1u: return (uintptr_t)image_draw;
    default: assert(!"unknown native call"); return 0;
    }
}
static void event(uint32_t e[5], uint32_t current, uint32_t target, unsigned code, uint32_t user) {
    e[0] = current; e[1] = target; e[2] = code; e[3] = user; e[4] = 77;
}
static void reset(void) {
    forbid_page_coords=tree_calls=0;
    memset(objects, 0, sizeof(objects));
    uint32_t ids[5] = {OWNER, PAGE, FOREGROUND, LIST, NEW_LIST};
    for (unsigned i = 0; i < 5; ++i) { objects[i].id = ids[i]; objects[i].live = 1; }
    object(PAGE)->parent = OWNER; object(FOREGROUND)->parent = PAGE;
    object(LIST)->clz = object(NEW_LIST)->clz = 0x2cd2024cu;
    object(OWNER)->d[0][0] = P67_SCROLL_CB;
    object(OWNER)->d[0][1] = PAGE;
    object(OWNER)->count = 1;
    root_slot = LIST; foreground_slot = FOREGROUND; image_slot = moving = 0;
    p67_owner = p67_page = p67_draw_owner = p67_generation = p67_busy = p67_wake_pending = 0;
    p67_blur_session_clear(&p67_launcher_session,1u);
    p67_blur_entry_scoped=p67_blur_allowed_entries=0;
    p67_blur_pending=p67_blur_armed=p67_blur_request_wake=p67_blur_demand=0;
    redraw_requests = 0; redraw_result = 0;
    p67_path_index = 0; memset(p67_path, 0, sizeof(p67_path));
    strcpy(p67_path[0], "/data/static.bin");
    add_calls = fail_add = mutate_binding = preparations = draws = 0; memset(forwards, 0, sizeof(forwards));
    object(PAGE)->bounds[0] = 0; object(PAGE)->bounds[2] = 335; object(PAGE)->bounds[3] = 479;
    for (unsigned i = 0; i < 4; ++i) layer_clip[i] = (uint32_t)(expected_clip[i] = (int32_t)object(PAGE)->bounds[i]);
}
static void check_blur_draw(void) {
    reset(); strcpy(p67_path[0],":blur"); p67_blur_source=0;
    rh_platform_backgrounds_poll(); assert(p67_draw_owner==LIST);
    uint32_t e[5],saved[4];
    unsigned initialized=image_inits,submitted=image_draws,copies=p67_blur_copies;
    uint32_t counted=p67_blur_draws;
    event(e,LIST,LIST,25,p67_generation);
    memcpy(saved,layer_clip,sizeof(saved));p67_event(e);
    assert(image_inits==initialized && !draws && !memcmp(saved,layer_clip,sizeof(saved)));
    p67_blur_source=0x20001000u;
    const unsigned positions[]={0,120,479,480};
    for(unsigned n=0;n<4;++n) {
        unsigned y=positions[n];
        object(PAGE)->y=y; object(PAGE)->bounds[3]=y+479;
        layer_clip[0]=(uint32_t)-20;layer_clip[1]=0;layer_clip[2]=360;layer_clip[3]=960;
        expected_clip[0]=0;expected_clip[1]=(int32_t)y;expected_clip[2]=335;expected_clip[3]=479;
        memcpy(saved,layer_clip,sizeof(saved));p67_event(e);
        unsigned visible=n<3?n+1:3;
        assert(draws==visible && image_draws==submitted+visible && image_inits==initialized+visible);
        assert(p67_blur_draws==counted+visible && p67_blur_copies==copies);
        assert(!memcmp(saved,layer_clip,sizeof(saved)));
        assert(last_image_descriptor[7]==p67_blur_source);
    }
    object(PAGE)->y=0;object(PAGE)->bounds[3]=479;
    event(e,LIST,LIST,25,p67_generation-1u);p67_event(e);assert(draws==3);
    event(e,LIST,LIST,24,p67_generation);p67_event(e);assert(draws==3);
    p67_blur_source=0;
}
/* Unlike draw event(), every gesture explicitly specifies the +16 parameter.
 * Non-NULL values are intentionally invalid addresses: never dereference them. */
static void scroll_event(unsigned code,uint32_t target,uint32_t page,uint32_t param) {
    uint32_t e[5];event(e,OWNER,target,code,page);e[4]=param;p67_scroll(e);
}
static void completed_capture(void) {
    p67_blur_source=0x20001000u;p67_blur_pending=p67_blur_armed=0;
}
static void bind_blur(void) {
    reset();strcpy(p67_path[0],":blur");p67_blur_source=0;
    object(PAGE)->y=480;
    rh_platform_backgrounds_poll();assert(p67_owner==OWNER);
}
static void check_blur_sessions(void) {
    const uint32_t params[]={0,0xdeadbeefu}; /* physical and animated/programmatic */
    for(unsigned n=0;n<2;++n) {
        bind_blur();uint32_t requested=p67_blur_request_serial;
        scroll_event(9,LIST,PAGE,params[n]);
        assert(p67_blur_request_serial==requested+1 && p67_blur_pending);
        assert(p67_launcher_session.generation==p67_generation);
        assert(p67_launcher_session.page==PAGE && p67_blur_allowed_entries==1);
        assert(forwards[9]==1); /* source0 retains native snapshot path */
        requested=p67_blur_request_serial;
        scroll_event(9,LIST,PAGE,params[!n]); /* still at closed, already latched */
        assert(p67_blur_request_serial==requested && forwards[9]==2);
        object(PAGE)->y=200;
        unsigned walks=tree_calls;
        for(unsigned frame=0;frame<128;++frame) scroll_event(12,LIST,PAGE,0);
        assert(tree_calls==walks && p67_launcher_session.page==PAGE);
        object(PAGE)->y=0; /* Bubbled list-content begin while the page is open. */
        scroll_event(9,LIST,PAGE,0xdeadbeefu);
        assert(p67_blur_request_serial==requested && p67_blur_pending && p67_blur_armed);
        assert(p67_blur_allowed_entries==1 && p67_launcher_session.page==PAGE);
        object(PAGE)->y=200;
        for(unsigned failed=0;failed<2;++failed) {
            if(failed) completed_capture();
            for(unsigned p=0;p<2;++p) {
                scroll_event(9,FOREGROUND,PAGE,params[p]);
                assert(p67_blur_request_serial==requested && p67_blur_allowed_entries==1);
            }
        }
        /* Mid-drag END(indev), END(NULL), reversal/newtouch do not reset provenance. */
        scroll_event(11,LIST,PAGE,0xdeadbeefu);scroll_event(11,LIST,PAGE,0);
        assert(p67_launcher_session.page==PAGE && p67_blur_allowed_entries==1);
        scroll_event(9,LIST,PAGE,0);assert(p67_blur_request_serial==requested);
        object(PAGE)->y=0;
        scroll_event(11,LIST,PAGE,0);assert(p67_launcher_session.page==PAGE);
        unsigned prepared=preparations,forwarded=forwards[9];
        scroll_event(9,LIST,PAGE,0); /* Existing launcher native9skip remains unchanged. */
        assert(p67_blur_request_serial==requested && p67_blur_allowed_entries==1);
        assert(preparations==prepared+1 && forwards[9]==forwarded);
        uintptr_t source=p67_blur_source;unsigned copies=p67_blur_copies;
        scroll_event(9,OWNER,PAGE,params[n]); /* open->watchface exit: invalidate, not request */
        assert(p67_blur_request_serial==requested+1 && !p67_blur_pending && !p67_blur_armed);
        assert(!p67_launcher_session.page && !p67_blur_allowed_entries);
        assert(p67_blur_source==source && p67_blur_copies==copies);
        requested=p67_blur_request_serial;
        scroll_event(9,LIST,PAGE,params[!n]);
        object(PAGE)->y=200;scroll_event(9,LIST,PAGE,0);
        assert(p67_blur_request_serial==requested && !p67_blur_pending);
        assert(p67_blur_source==source && p67_blur_copies==copies);
        /* No END needed: closed SCROLL(12) cancels, next entry is fresh. */
        object(PAGE)->y=480;scroll_event(9,LIST,PAGE,params[n]);
        assert(p67_blur_request_serial==requested+1 && p67_blur_pending);
        requested=p67_blur_request_serial;scroll_event(12,LIST,PAGE,0);
        assert(!p67_launcher_session.page && !p67_blur_allowed_entries && !p67_blur_pending);
        assert(p67_blur_request_serial==requested+1);
        requested=p67_blur_request_serial;scroll_event(9,LIST,PAGE,params[!n]);
        assert(p67_blur_request_serial==requested+1 && p67_blur_pending);
        scroll_event(11,LIST,PAGE,0xdeadbeefu); /* closed END, param is irrelevant */
        assert(!p67_launcher_session.page && !p67_blur_pending);
    }
    /* Binding generation changes at partial cannot turn an unknown begin into entry. */
    bind_blur();scroll_event(9,LIST,PAGE,0);object(PAGE)->y=200;++p67_generation;
    scroll_event(9,LIST,PAGE,0xdeadbeefu);
    assert(!p67_launcher_session.page && !p67_blur_allowed_entries && !p67_blur_pending);
    uint32_t requested=p67_blur_request_serial;
    scroll_event(9,LIST,PAGE,0);assert(p67_blur_request_serial==requested);
    object(PAGE)->y=480;scroll_event(9,LIST,PAGE,0xdeadbeefu);
    assert(p67_blur_request_serial==requested+1 && p67_launcher_session.generation==p67_generation);
    /* Wrong user-data/current owner never seeds a session. */
    bind_blur();requested=p67_blur_request_serial;scroll_event(9,LIST,FOREGROUND,0);
    uint32_t e[5];event(e,LIST,LIST,9,PAGE);p67_scroll(e);
    assert(p67_blur_request_serial==requested && !p67_launcher_session.page);
    /* Detach and either bound subscriber DELETE revoke this page's budget. */
    for(unsigned which=0;which<3;++which) {
        bind_blur();scroll_event(9,LIST,PAGE,0);completed_capture();
        uintptr_t source=p67_blur_source;
        if(!which) p67_detach();
        else {
            uint32_t obj=which==1?OWNER:LIST;
            event(e,obj,obj,36,p67_generation);p67_event(e);
        }
        assert(!p67_launcher_session.page && !p67_blur_allowed_entries);
        assert(p67_blur_source==source);
    }
    /* Residual owner userdata after root DELETE/root-slot replacement, and a
     * dead PAGE at begin, are never permission to read the PAGE coordinate. */
    for(unsigned which=0;which<3;++which) {
        bind_blur();scroll_event(9,LIST,PAGE,0);completed_capture();
        if(which==0) {event(e,LIST,LIST,36,p67_generation);p67_event(e);}
        if(which==1) root_slot=NEW_LIST;
        if(which==2) object(PAGE)->live=0;
        forbid_page_coords=1;
        scroll_event(9,LIST,PAGE,0);
        if(which!=2) scroll_event(12,LIST,PAGE,0);
        forbid_page_coords=0;
        assert(!p67_launcher_session.page && !p67_blur_allowed_entries);
    }
    p67_blur_source=0;
}
int main(void) {
    uint32_t e[5];
    reset(); fail_add = 1; rh_platform_backgrounds_poll();
    assert(!p67_owner && object(OWNER)->count == 1 && !object(LIST)->count);
    assert(object(OWNER)->d[0][0] == P67_SCROLL_CB);
    reset(); fail_add = 2; rh_platform_backgrounds_poll();
    assert(!p67_owner && object(OWNER)->count == 1 && !object(LIST)->count);
    assert(object(OWNER)->d[0][0] == P67_SCROLL_CB);
    reset(); mutate_binding = 1; rh_platform_backgrounds_poll();
    assert(!p67_owner && object(OWNER)->count == 1 && !object(LIST)->count);
    assert(object(OWNER)->d[0][0] == P67_SCROLL_CB);
    reset(); image_slot = 123; rh_platform_backgrounds_poll(); assert(!add_calls);
    image_slot = 0; moving = 1; rh_platform_backgrounds_poll(); assert(!add_calls);
    moving = 0; rh_platform_backgrounds_poll();
    assert(p67_owner == OWNER && p67_page == PAGE && p67_draw_owner == LIST);
    assert(object(OWNER)->d[0][1] == PAGE && !object(OWNER)->d[0][2]);
    assert(object(OWNER)->d[0][0] == (uint32_t)(uintptr_t)p67_scroll);
    /* Bubbled event: targets unrelated; native user data remains PAGE. */
    object(OWNER)->y = 480; object(PAGE)->y = 0;
    event(e, OWNER, LIST, 0x8009u, PAGE); p67_scroll(e);
    assert(moving && preparations == 1 && last_prepared == LIST && !forwards[9]);
    event(e, OWNER, LIST, 12, PAGE); p67_scroll(e); assert(forwards[12] == 1);
    event(e, OWNER, LIST, 11, PAGE); p67_scroll(e); assert(forwards[11] == 1 && !moving);
    object(PAGE)->y = 480; object(OWNER)->y = 0;
    event(e, OWNER, LIST, 9, PAGE); p67_scroll(e);
    assert(moving && preparations == 1); moving = 0;
    event(e, OWNER, LIST, 9, FOREGROUND); p67_scroll(e); assert(forwards[9] == 1);
    /* Native extended clip can exceed screen/page. Submission narrows it,
     * destination stays fixed, then the previous clip is restored exactly. */
    object(PAGE)->y = 120; object(PAGE)->bounds[3] = 599;
    layer_clip[0] = (uint32_t)-20; layer_clip[1] = 100; layer_clip[2] = 360; layer_clip[3] = 600;
    expected_clip[0] = 0; expected_clip[1] = 120; expected_clip[2] = 335; expected_clip[3] = 479;
    uint32_t saved[4]; memcpy(saved, layer_clip, sizeof(saved));
    event(e, LIST, LIST, 0x8019u, p67_generation); p67_event(e); assert(draws == 1);
    assert(!memcmp(saved, layer_clip, sizeof(saved)));
    object(PAGE)->y = 480; p67_event(e); assert(draws == 1);
    assert(!memcmp(saved, layer_clip, sizeof(saved)));
    uint32_t stale_generation = p67_generation;
    event(e, OWNER, OWNER, 36, p67_generation); p67_event(e);
    assert(!p67_owner && !p67_page && p67_generation != stale_generation);
    object(OWNER)->live = 0;
    event(e, LIST, LIST, 25, stale_generation); p67_event(e); assert(draws == 1);
    rh_platform_backgrounds_poll(); assert(!p67_draw_owner && !object(LIST)->count);
    /* Fresh owner/address reuse must bind a new generation. */
    object(OWNER)->live = 1; object(OWNER)->count = 1;
    object(OWNER)->d[0][0] = P67_SCROLL_CB;
    rh_platform_backgrounds_poll(); assert(p67_owner == OWNER && p67_generation != stale_generation);
    event(e, LIST, LIST, 36, p67_generation); p67_event(e); object(LIST)->live = 0;
    root_slot = NEW_LIST; rh_platform_backgrounds_poll();
    assert(p67_owner == OWNER && p67_draw_owner == NEW_LIST);
    struct rh_backgrounds bg; memset(&bg, 0, sizeof(bg));
    moving = 1; assert(rh_platform_backgrounds_commit(&bg) == -2145 && p67_owner);
    moving = 0; assert(!rh_platform_backgrounds_commit(&bg));
    assert(!p67_owner && !p67_draw_owner && !object(NEW_LIST)->count);
    assert(object(OWNER)->count == 1 && object(OWNER)->d[0][0] == P67_SCROLL_CB);
    assert(redraw_requests);
    redraw_result = -1;
    assert(!rh_platform_backgrounds_commit(&bg) && p67_wake_pending);
    redraw_result = 0; rh_platform_backgrounds_poll(); assert(!p67_wake_pending);
    check_blur_draw();
    check_blur_sessions();
    puts("p67 live launcher mocked native lifecycle/GPU blur drawing/sessions passed");
    return 0;
}
