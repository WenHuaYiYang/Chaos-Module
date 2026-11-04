#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stddef.h>
#include "resource_hook_background.h"
#define RH_TARGET_1043 1
#define RH_RELOAD_LIMIT 1024u
/* Host seam: keep the file mode observable without external platform declarations. */
#define RH_FILE_READ 1
#define RH_FW_OBJECT_TREE_WALK 1u
#define RH_FW_IMAGE_GET_INFO 2u
#define RH_FW_IMAGE_CLASS 0x2cce61ecu
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
struct object { uint32_t id, parent, clz, y, bounds[4], count, d[8][3]; int live, off_tree; };
static struct object objects[5], ancestors[16];
static unsigned char moving;
static uint32_t surface_root, surface_image;
static int hidden_root, hidden_image, capture_fail;
static unsigned cleanup, selected, first_enter, cache_reuses, captures;
static unsigned root_hides, root_shows, image_hides, image_shows;
static unsigned buffer_releases, pool_releases, image_deletes, end_bookkeeping;
static unsigned begin_bookkeeping, end_reentry;
static int native_buffer, reenter_on_end, delete_on_end;
static uint32_t timer_resume_hook;
static uint32_t edit_container_slot;
static unsigned edit_opa_calls;
static uint32_t last_edit_opa_obj, last_edit_opa_val;
static void set_bg_opa(uint32_t obj, uint32_t val, uint32_t selector) {
    (void)selector;
    ++edit_opa_calls;
    last_edit_opa_obj = obj;
    last_edit_opa_val = val;
}
static unsigned timer_resets, timer_resumes, predelete_reentry;
static int reenter_before_delete;
static uint32_t populated;
static int delete_on_flag, delete_on_remove, shared_mode;
static uint32_t root_slot, image_slot, foreground_slot;
static unsigned add_calls, fail_add, mutate_binding, forwards[64], preparations, draws;
static int forbid_mutation;
static unsigned tree_calls,parent_calls,forbid_page_coords;
static uint32_t layer_clip[4];
static int32_t expected_clip[4];
static uint32_t last_prepared, last_draw_source;
static unsigned image_inits, image_draws;
static uint32_t last_image_descriptor[28];
static struct object *object(uint32_t id) {
    for (unsigned i = 0; i < 5; ++i) if (objects[i].id == id) return &objects[i];
    for (unsigned i = 0; i < 16; ++i) if (ancestors[i].id == id) return &ancestors[i];
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
            assert(s->live);
            return index == 1 ? &s->y : &s->bounds[index];
        }
    }
    assert(!"unknown word"); return 0;
}
static uint32_t load32(uint32_t a) {
    if(shared_mode) {
      if(a==0x2010fe50u) return LIST;
      if(a==0x20110ef4u) return FOREGROUND;
      if(a==0x201100e0u || a==0x2010fe64u || a==0x20110ed4u || a==0x201100ccu) return 0;
    }
    if(a==0x2010fe50u || a==0x20110ef4u || a==0x201100e0u) return surface_root;
    if(a==0x2010fe64u || a==0x20110ed4u || a==0x201100ccu) return surface_image;
    if(a==0x20110ee8u || a==0x201100dcu) return populated;
    if(a==0x2010fe80u) return edit_container_slot;
    if(a==0x20103218u) return timer_resume_hook;
    if (a == 0x201102ecu) return root_slot;
    if (a == 0x201102dcu) return image_slot;
    if (a == 0x201102f4u) return foreground_slot;
    if (a == RH_FW_HEADER_CACHE_SLOT || a == RH_FW_IMAGE_CACHE_SLOT) return 0;
    return object(a)->clz;
}
static int cache_valid(uint32_t a, uint32_t b) { (void)a; (void)b; return 1; }
static int retire_cache(uint32_t a, uint32_t b, const struct rh_mapping_view *v, const char *p) {
    (void)a; (void)b; (void)v; (void)p; return 0;
}
static int rh_platform_open(const char *p, int flags) { (void)p; assert(flags == RH_FILE_READ); return 1; }
static void rh_platform_close(int fd) { assert(fd == 1); }
static unsigned redraw_requests;
static int redraw_result;
static int rh_platform_request_full_redraw(void) { ++redraw_requests; return redraw_result; }
#include "../src/p67_backgrounds.inc"
static void tree(uint32_t root, int (*cb)(uint32_t, void *), void *cookie) {
    assert(root == 0); ++tree_calls;
    for (unsigned i = 0; i < 5; ++i)
        if (objects[i].live && !objects[i].off_tree && cb(objects[i].id, cookie) == 2) break;
}
static uint32_t count(uint32_t o) { return object(o)->count; }
static uint32_t *get(uint32_t o, uint32_t index) {
    struct object *s = object(o); assert(s->live && index < s->count); return s->d[index];
}
static uint32_t parent(uint32_t o) {
    assert(object(o)->live); ++parent_calls; return object(o)->parent;
}
static uint32_t *add(uint32_t o, uint32_t cb, uint32_t filter, uint32_t user) {
    struct object *s = object(o); assert(s->live && !forbid_mutation);
    if (++add_calls == fail_add) return 0;
    if (mutate_binding && add_calls == 2) object(OWNER)->d[0][1] = FOREGROUND;
    assert(s->count < 8); uint32_t *d = s->d[s->count++];
    d[0] = cb; d[1] = user; d[2] = filter; return d;
}
static int remove_event(uint32_t o, uint32_t cb) {
    struct object *s = object(o); assert(s->live && !forbid_mutation);
    for (unsigned i = 0; i < s->count; ++i) if (s->d[i][0] == cb) {
        memmove(&s->d[i], &s->d[i + 1], (s->count - i - 1) * sizeof(s->d[0]));
        --s->count; return 1;
    }
    return 0;
}
static void original(void *e) {
    unsigned code = *(uint16_t *)((unsigned char *)e + 8) & 0x7fffu; assert(code < 64);
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
    last_draw_source=((const uint32_t *)d)[30];
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
    for(unsigned i=0;i<4;++i) assert((int32_t)layer_clip[i]==expected_clip[i]);
    last_draw_source=descriptor[7];
    memcpy(last_image_descriptor,d,sizeof(last_image_descriptor));
    ++image_draws; ++draws;
}
static int image_info(const char *p, void *out) { (void)out; assert(p[0] && !rh_background_is_blur(p)); return 1; }
static void flag_add(uint32_t o,uint32_t f) { assert(f==1 && !forbid_mutation);
 if(delete_on_flag && o==NEW_LIST) {
  delete_on_flag=0;
  uint32_t e[5]={LIST,LIST,36,p67_surfaces[selected].generation,77};
  p67_surface_event(e);object(LIST)->live=0;surface_root=0;
 }
 if(o==LIST) {++root_hides;hidden_root=1;}
 else {assert(o==NEW_LIST);++image_hides;hidden_image=1;} }
static void flag_remove(uint32_t o,uint32_t f) { assert(f==1 && !forbid_mutation);
 if(delete_on_remove && o==NEW_LIST) {
  delete_on_remove=0;uint32_t e[5]={LIST,LIST,36,p67_surfaces[selected].generation,77};
  p67_surface_event(e);object(LIST)->live=0;surface_root=0;
 }
 if(o==LIST) {++root_shows;hidden_root=0;}
 else {assert(o==NEW_LIST);++image_shows;hidden_image=0;} }
static int flag_has(uint32_t o,uint32_t f) { assert(f==1);if(shared_mode && o==FOREGROUND)return 0;if(o==LIST)return hidden_root;assert(o==NEW_LIST);return hidden_image; }
static void surface_original(void *e) {
 unsigned code=*(uint16_t *)((char *)e+8)&0x7fff; assert(code<64); ++forwards[code];
 if(code==9) {
  ++begin_bookkeeping;
  if(selected==2 && !first_enter++) {
   if(surface_image) ++cache_reuses;
  } else if(!capture_fail) {
   if(selected!=1 && native_buffer) ++buffer_releases;
   ++captures;surface_image=NEW_LIST;native_buffer=1;object(NEW_LIST)->live=1;
  }
  if(surface_image && native_buffer && !hidden_root && hidden_image) {
   flag_remove(NEW_LIST,1);flag_add(LIST,1);
  }
 }
 if(code==11 && p67_terminal(&p67_surfaces[selected],PAGE)) {
  ++end_bookkeeping;
  if(selected==1) {
   /* y=0 queues its two bookkeeping callbacks and reset/resume only invokes
    * the global wake hook. All non-NULL hooks (including the audited native
    * hook outside the target allowlist) and outer-end systray callbacks must
    * still observe the original native flags before image DELETE. */
   if(object(PAGE)->y==0) {
    timer_resets+=1;timer_resumes+=1;
    if(timer_resume_hook && surface_image)
     assert(hidden_root && !hidden_image);
    if(reenter_before_delete) {
     reenter_before_delete=0;++predelete_reentry;
     p67_surface_scroll_for(&p67_surfaces[selected],e);
    }
   } else if(surface_image) assert(hidden_root && !hidden_image);
   /* C526B88 DELETE reads source independently of visibility, releases its
    * native pool slot, clears ED4/ED8 and restores root unconditionally. */
   if(surface_image) {
    assert(object(NEW_LIST)->live);
    ++image_deletes;++pool_releases;++cleanup;
    object(NEW_LIST)->live=0;surface_image=0;native_buffer=0;
    flag_remove(LIST,1);hidden_image=1;
   }
  } else {
   /* C5036C4/C50ABB4: visibility predicate does NOT guard the buffer free. */
   if(hidden_root && !hidden_image) {flag_remove(LIST,1);flag_add(NEW_LIST,1);}
   if(native_buffer) {++buffer_releases;++cleanup;native_buffer=0;}
  }
  if(reenter_on_end) {
   reenter_on_end=0;++end_reentry;p67_surface_scroll_for(&p67_surfaces[selected],e);
  }
  if(delete_on_end) {
   uint32_t gone[5]={LIST,LIST,36,p67_surfaces[selected].generation,77};
   delete_on_end=0;p67_surface_event(gone);object(LIST)->live=0;surface_root=0;
  }
 }
}
static unsigned native_cc_calls,native_notify_calls,native_widget_calls;
static void original_cc(void *e) { ++native_cc_calls;surface_original(e); }
static void original_notify(void *e) { ++native_notify_calls;surface_original(e); }
static void original_widget(void *e) { ++native_widget_calls;surface_original(e); }
static uintptr_t native(uint32_t a) {
    switch (a) {
    case 0x0c5036c5u:return (uintptr_t)original_cc;
    case 0x0c527bbdu:return (uintptr_t)original_notify;
    case 0x0c50abb5u:return (uintptr_t)original_widget;
    case 0x0c588961u:return (uintptr_t)flag_add;
    case 0x0c5891b1u:return (uintptr_t)flag_remove;
    case 0x0c5881c1u:return (uintptr_t)flag_has;
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
    case 0x0c5883f9u: return (uintptr_t)set_bg_opa;
    default: assert(!"unknown native call"); return 0;
    }
}
static void event(uint32_t e[5], uint32_t current, uint32_t target, unsigned code, uint32_t user) {
    e[0] = current; e[1] = target; e[2] = code; e[3] = user; e[4] = 77;
}
/* Model descriptor ALL delivery: each callback receives its own userdata,
 * including the native page binding (not the adapter generation) on owner. */
static void dispatch(struct p67_surface *s, uint32_t obj, unsigned code) {
    struct object *o=object(obj);
    for(unsigned n=0;n<o->count;++n) {
        uint32_t *d=o->d[n], e[5];
        if(d[2] && d[2]!=code) continue;
        event(e,obj,obj,code,d[1]);
        e[4]=code==25?77u:0u;
        if(d[0]==p67_scroll_fn(s)) p67_surface_scroll_for(s,e);
        else { assert(d[0]==p67_event_fn(s)); p67_surface_event(e); }
    }
}
static void check_non_gestures(struct p67_surface *s) {
    const unsigned codes[]={1,20,21,23,24,25,26,27,28,32,33,47};
    struct p67_surface before=*s;
    struct object saved_objects[5];
    memcpy(saved_objects,objects,sizeof(objects));
    unsigned adds=add_calls;
    /* A newly populated list must not register from an ordinary/render ALL. */
    populated=FOREGROUND;
    object(FOREGROUND)->clz=0x2cdba28cu;object(FOREGROUND)->parent=LIST;
    memcpy(saved_objects,objects,sizeof(objects));
    /* Stronger than a render-only guard: all unaudited ALL events, including
     * SIZE_CHANGED/STYLE_CHANGED/GET_SELF_SIZE, must be mutation-free. */
    forbid_mutation=1;
    for(unsigned n=0;n<sizeof(codes)/sizeof(codes[0]);++n) {
        unsigned calls=forwards[codes[n]];
        dispatch(s,OWNER,codes[n]);
        assert(forwards[codes[n]]==calls+1);
        assert(!memcmp(&before,s,sizeof(before)));
        assert(!memcmp(saved_objects,objects,sizeof(objects)));
        assert(add_calls==adds && !hidden_root && hidden_image);
    }
    /* Render phases traverse owner ALL before the custom root draw. */
    unsigned rendered=draws;
    for(unsigned code=23;code<=28;++code) {
        dispatch(s,OWNER,code);dispatch(s,LIST,code);
        assert(!memcmp(&before,s,sizeof(before)));
    }
    assert(draws==rendered+1);
    forbid_mutation=0;populated=0;
}
static void check_steady_scroll(struct p67_surface *s) {
    struct p67_surface before=*s;
    struct object saved_objects[5];
    memcpy(saved_objects,objects,sizeof(objects));
    unsigned walks=tree_calls, parents=parent_calls, calls=forwards[12], adds=add_calls;
    unsigned snapshots=captures, released=cleanup;
    assert(s->moving);
    forbid_mutation=1;
    for(unsigned n=0;n<128;++n) {
        dispatch(s,OWNER,12);
        assert(forwards[12]==calls+n+1);
        assert(tree_calls==walks && parent_calls==parents && add_calls==adds);
        assert(!memcmp(&before,s,sizeof(before)));
        assert(!memcmp(saved_objects,objects,sizeof(objects)));
        assert(!hidden_root && hidden_image);
        assert(captures==snapshots && cleanup==released);
    }
    forbid_mutation=0;
}
static void reset(void) {
    forbid_mutation=forbid_page_coords=0; tree_calls=parent_calls=0;
    root_hides=root_shows=image_hides=image_shows=0;
    buffer_releases=pool_releases=image_deletes=end_bookkeeping=begin_bookkeeping=end_reentry=0;
    native_buffer=1;reenter_on_end=delete_on_end=delete_on_flag=delete_on_remove=0;
    timer_resume_hook=0;
    timer_resets=timer_resumes=predelete_reentry=0;reenter_before_delete=0;
    memset(objects, 0, sizeof(objects));memset(ancestors, 0, sizeof(ancestors));
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
static void check_terminal_ends(void) {
 for(unsigned i=0;i<3;++i) for(unsigned outer=0;outer<2;++outer)
 for(unsigned blur=0;blur<2;++blur) {
  reset();selected=i;shared_mode=0;populated=0;first_enter=cache_reuses=captures=cleanup=0;
  p67_path[0][0]=0;
  struct p67_surface *s=&p67_surfaces[i];
  memset(&s->owner,0,sizeof(*s)-offsetof(struct p67_surface,owner));
  strcpy(s->path,blur?":blur":"/data/static.bin");p67_blur_source=0x20001000u;
  surface_root=LIST;surface_image=NEW_LIST;hidden_root=0;hidden_image=1;capture_fail=0;
  object(LIST)->clz=0x2cdba8b4u;object(NEW_LIST)->clz=RH_FW_IMAGE_CLASS;
  object(LIST)->parent=object(NEW_LIST)->parent=PAGE;
  object(OWNER)->d[0][0]=s->callback;p67_surface_poll(s);assert(s->owner==OWNER);
  if(i==1) object(PAGE)->y=(uint32_t)-480;
  else object(PAGE)->bounds[0]=i==0?(uint32_t)-336:336;
  uint32_t requested=p67_blur_request_serial;
  dispatch(s,OWNER,9);
  assert(s->moving && s->swapped && begin_bookkeeping==1);
  assert(p67_blur_request_serial==requested+blur);
  /* End in transit still restores/reswaps; reversal begin still forwards. */
  if(i==1) object(PAGE)->y=(uint32_t)-100;
  else object(PAGE)->bounds[0]=i==0?(uint32_t)-100:100;
  unsigned released=buffer_releases, pooled=pool_releases;
  root_hides=root_shows=image_hides=image_shows=0;
  dispatch(s,OWNER,11);
  assert(s->moving && s->swapped && !hidden_root && hidden_image);
  assert(!cleanup && !end_bookkeeping && buffer_releases==released && pool_releases==pooled);
  assert(root_hides==1 && root_shows==1 && image_hides==1 && image_shows==1);
  if(i==1) object(PAGE)->y=0;else object(PAGE)->bounds[0]=0;
  dispatch(s,OWNER,9);
  assert(begin_bookkeeping==2 && s->swapped);
  assert(p67_blur_request_serial==requested+2*blur);
  if(outer) {
   if(i==1) object(PAGE)->y=(uint32_t)-480;
   else object(PAGE)->bounds[0]=i==0?(uint32_t)-336:336;
  }
  released=buffer_releases;pooled=pool_releases;
  root_hides=root_shows=image_hides=image_shows=0;reenter_on_end=1;
  unsigned forwarded=forwards[11];
  dispatch(s,OWNER,11);
  assert(forwards[11]==forwarded+1 && end_reentry==1 && end_bookkeeping==1);
  assert(cleanup==1 && !s->moving && !s->swapped && !s->busy && !hidden_root);
  assert(p67_blur_request_serial==requested+2*blur);
  if(i==1) {
   assert(image_deletes==1 && pool_releases==pooled+1 && buffer_releases==released);
   assert(!surface_image && !object(NEW_LIST)->live);
   assert(root_hides==outer && root_shows==1 && image_shows==outer && !image_hides);
   assert(timer_resets==!outer && timer_resumes==!outer);
  } else {
   assert(buffer_releases==released+1 && pool_releases==pooled && !image_deletes);
   assert(surface_image==NEW_LIST && hidden_image);
   assert(!root_hides && !root_shows && !image_hides && !image_shows);
  }
  /* Repeated end cannot release the same native buffer/pool twice or add
   * another visibility round trip after native cleanup. */
  unsigned freed=buffer_releases, returned=pool_releases, deleted=image_deletes;
  unsigned hides=root_hides, shows=root_shows, ih=image_hides, is=image_shows;
  dispatch(s,OWNER,11);assert(cleanup==1 && end_bookkeeping==2);
  assert(buffer_releases==freed && pool_releases==returned && image_deletes==deleted);
  assert(root_hides==hides && root_shows==shows && image_hides==ih && image_shows==is);
  assert(!s->moving && !s->swapped && !hidden_root);
  p67_surface_detach(s);s->path[0]=0;
 }
 /* Inconsistent final flags take the conservative restoration path. Native
  * DELETE invalidates both optimized and fallback bindings before post-end work. */
 for(unsigned i=0;i<3;++i) for(unsigned fallback=0;fallback<2;++fallback) {
  reset();selected=i;shared_mode=0;populated=0;cleanup=0;
  struct p67_surface *s=&p67_surfaces[i];
  memset(&s->owner,0,sizeof(*s)-offsetof(struct p67_surface,owner));
  strcpy(s->path,"/data/static.bin");
  s->owner=OWNER;s->page=PAGE;s->root=LIST;s->generation=1000+i;
  s->moving=s->swapped=1;
  surface_root=LIST;surface_image=NEW_LIST;hidden_root=(int)fallback;hidden_image=1;
  object(LIST)->clz=0x2cdba8b4u;object(NEW_LIST)->clz=RH_FW_IMAGE_CLASS;
  object(LIST)->parent=object(NEW_LIST)->parent=PAGE;
  uint32_t e[5];event(e,OWNER,OWNER,11,PAGE);delete_on_end=1;
  p67_surface_scroll_for(s,e);
  assert(cleanup==1 && !s->root && !s->moving && !s->busy && s->dirty);
  assert(!root_hides);
  assert(root_shows==(i==1 || fallback) && image_shows==fallback);
  assert(s->generation!=1000+i && !object(LIST)->live);
  p67_surface_detach(s);s->path[0]=0;
 }
 /* Notify center end admits only NULL wake hooks. Both audited native and
  * unknown non-NULL hooks retain the original swap without expanding the
  * target allowlist. Pool release/DELETE remains exactly once on reentry. */
 const uint32_t hooks[]={0,0x0c4b8411u,0xdeadbeefu};
 for(unsigned n=0;n<3;++n) {
  reset();selected=1;shared_mode=0;populated=0;cleanup=0;
  struct p67_surface *s=&p67_surfaces[1];
  memset(&s->owner,0,sizeof(*s)-offsetof(struct p67_surface,owner));
  strcpy(s->path,"/data/static.bin");
  s->owner=OWNER;s->page=PAGE;s->root=LIST;s->generation=1100+n;s->moving=s->swapped=1;
  surface_root=LIST;surface_image=NEW_LIST;hidden_root=0;hidden_image=1;
  object(LIST)->clz=0x2cdba8b4u;object(NEW_LIST)->clz=RH_FW_IMAGE_CLASS;
  object(LIST)->parent=object(NEW_LIST)->parent=PAGE;
  timer_resume_hook=hooks[n];reenter_before_delete=1;
  uint32_t e[5];event(e,OWNER,OWNER,11,PAGE);
  p67_surface_scroll_for(s,e);
  assert(predelete_reentry==1 && forwards[11]==1 && end_bookkeeping==1);
  assert(timer_resets==1 && timer_resumes==1 && image_deletes==1 && pool_releases==1);
  assert(!buffer_releases && cleanup==1 && !surface_image && !object(NEW_LIST)->live);
  assert(root_hides==(n!=0) && image_shows==(n!=0) && root_shows==1 && !image_hides);
  assert(!hidden_root && !s->swapped && !s->moving && !s->busy);
  p67_surface_detach(s);s->path[0]=0;
 }
 p67_blur_source=0;
}
/* Gesture parameters are opaque booleans, including a deliberately invalid
 * stack-animation stand-in. Never inherit draw event()'s layer value here. */
static void scroll_event(struct p67_surface *s,unsigned code,uint32_t target,
                         uint32_t page,uint32_t param) {
 uint32_t e[5];event(e,OWNER,target,code,page);e[4]=param;p67_surface_scroll_for(s,e);
}
static void completed_capture(void) {
 p67_blur_source=0x20001000u;p67_blur_pending=p67_blur_armed=0;
}
static void position(struct p67_surface *s,int32_t value) {
 if(s==&p67_surfaces[1]) object(s->page)->y=(uint32_t)value;
 else object(s->page)->bounds[0]=(uint32_t)value;
}
static struct p67_surface *bind_blur(unsigned i) {
 reset();selected=i;shared_mode=0;populated=0;first_enter=cache_reuses=captures=cleanup=0;
 p67_path[0][0]=0;p67_blur_source=0;
 struct p67_surface *s=&p67_surfaces[i];
 memset(&s->owner,0,sizeof(*s)-offsetof(struct p67_surface,owner));
 strcpy(s->path,":blur");
 surface_root=LIST;surface_image=NEW_LIST;hidden_root=0;hidden_image=1;capture_fail=0;
 object(LIST)->clz=0x2cdba8b4u;object(NEW_LIST)->clz=RH_FW_IMAGE_CLASS;
 object(LIST)->parent=object(NEW_LIST)->parent=PAGE;
 object(OWNER)->d[0][0]=s->callback;p67_surface_poll(s);assert(s->owner==OWNER);
 position(s,p67_surface_closed(s));
 return s;
}
static void check_blur_sessions(void) {
 const uint32_t params[]={0,0xdeadbeefu};
 for(unsigned i=0;i<3;++i) for(unsigned n=0;n<2;++n) {
  struct p67_surface *s=bind_blur(i);unsigned bit=2u<<i;
  uint32_t requested=p67_blur_request_serial;
  scroll_event(s,9,LIST,PAGE,params[n]);
  assert(p67_blur_request_serial==requested+1 && p67_blur_pending);
  assert(s->session.page==PAGE && s->session.generation==s->generation);
  assert(p67_blur_allowed_entries==bit && s->moving);
  assert(forwards[9]==1 && begin_bookkeeping==1);
  requested=p67_blur_request_serial;
  scroll_event(s,9,LIST,PAGE,params[!n]); /* repeat before first coordinate update */
  assert(p67_blur_request_serial==requested && s->session.page==PAGE);
  position(s,i==0?-100:i==1?-100:100);
  completed_capture(); /* code12 never restores flags or walks the tree */
  unsigned walks=tree_calls,hides=root_hides,shows=root_shows,ih=image_hides,is=image_shows;
  for(unsigned frame=0;frame<128;++frame) scroll_event(s,12,LIST,PAGE,0);
  assert(tree_calls==walks && root_hides==hides && root_shows==shows);
  assert(image_hides==ih && image_shows==is && s->session.page==PAGE);
  p67_blur_source=0;p67_blur_pending=p67_blur_armed=1; /* initial request still pending/failed */
  position(s,0);scroll_event(s,9,LIST,PAGE,0xdeadbeefu); /* Bubbled content, not exit. */
  assert(p67_blur_request_serial==requested && p67_blur_pending && p67_blur_armed);
  assert(p67_blur_allowed_entries==bit && s->session.page==PAGE);
  position(s,i==0?-100:i==1?-100:100);
  for(unsigned success=0;success<2;++success) {
   if(success) completed_capture();
   for(unsigned p=0;p<2;++p) {
    scroll_event(s,9,FOREGROUND,PAGE,params[p]); /* partial snap/reversal/newtouch */
    assert(p67_blur_request_serial==requested && p67_blur_allowed_entries==bit);
   }
  }
  scroll_event(s,11,LIST,PAGE,0xdeadbeefu);scroll_event(s,11,LIST,PAGE,0);
  assert(s->session.page==PAGE && p67_blur_allowed_entries==bit && s->moving);
  position(s,0);scroll_event(s,11,LIST,PAGE,0); /* open END does not revoke entry */
  assert(s->session.page==PAGE && p67_blur_allowed_entries==bit);
  unsigned forwarded=forwards[9];scroll_event(s,9,LIST,PAGE,0);
  assert(forwards[9]==forwarded+1 && p67_blur_request_serial==requested);
  assert(s->session.page==PAGE && p67_blur_allowed_entries==bit);
  uintptr_t source=p67_blur_source;unsigned copies=p67_blur_copies;
  scroll_event(s,9,OWNER,PAGE,params[n]);
  assert(p67_blur_request_serial==requested+1 && !p67_blur_pending && !p67_blur_armed);
  assert(!s->session.page && !p67_blur_allowed_entries);
  assert(p67_blur_source==source && p67_blur_copies==copies);
  requested=p67_blur_request_serial;scroll_event(s,9,LIST,PAGE,params[!n]);
  position(s,i==0?-100:i==1?-100:100);scroll_event(s,9,LIST,PAGE,0);
  assert(p67_blur_request_serial==requested && !p67_blur_pending);
  assert(p67_blur_source==source && p67_blur_copies==copies);
  /* Cancellation without END: closed 12 revokes and permits the next entry. */
  position(s,p67_surface_closed(s));scroll_event(s,9,LIST,PAGE,params[n]);
  assert(p67_blur_request_serial==requested+1 && s->session.page==PAGE);
  requested=p67_blur_request_serial;scroll_event(s,12,LIST,PAGE,0);
  assert(!s->session.page && !p67_blur_allowed_entries && !p67_blur_pending);
  assert(p67_blur_request_serial==requested+1);
  requested=p67_blur_request_serial;scroll_event(s,9,LIST,PAGE,params[!n]);
  assert(p67_blur_request_serial==requested+1 && s->session.page==PAGE);
  scroll_event(s,11,LIST,PAGE,0xdeadbeefu);
  assert(!s->session.page && !p67_blur_pending);
  assert(forwards[9]==begin_bookkeeping); /* Every native begin still forwarded. */
  p67_surface_detach(s);s->path[0]=0;
 }
 for(unsigned i=0;i<3;++i) {
  /* Generation change at partial is fail closed; only exact closed can start. */
  struct p67_surface *s=bind_blur(i);
  scroll_event(s,9,LIST,PAGE,0);position(s,i==2?100:-100);++s->generation;
  scroll_event(s,9,LIST,PAGE,0xdeadbeefu);
  assert(!s->session.page && !p67_blur_allowed_entries && !p67_blur_pending);
  uint32_t requested=p67_blur_request_serial;
  scroll_event(s,9,LIST,PAGE,0);assert(p67_blur_request_serial==requested);
  position(s,p67_surface_closed(s));scroll_event(s,9,LIST,PAGE,0xdeadbeefu);
  assert(p67_blur_request_serial==requested+1 && s->session.generation==s->generation);
  p67_surface_detach(s);s->path[0]=0;
  for(unsigned which=0;which<4;++which) {
   s=bind_blur(i);scroll_event(s,9,LIST,PAGE,0);completed_capture();
   uintptr_t source=p67_blur_source;
   if(!which) p67_surface_detach(s);
   else {
    uint32_t obj=which==1?OWNER:which==2?LIST:FOREGROUND,e[5];
    if(which==3) s->list=obj;
    uint32_t generation=s->generation;
    requested=p67_blur_request_serial;
    event(e,obj,obj,36,generation);p67_surface_event(e);
    if(which==3) {
     assert(s->generation==generation && s->session.page==PAGE);
     assert(p67_blur_allowed_entries==(2u<<i) && p67_blur_request_serial==requested);
    }
   }
   if(which!=3) assert(!s->session.page && !p67_blur_allowed_entries);
   assert(p67_blur_source==source);
   p67_surface_detach(s);s->path[0]=0;
  }
 }
 /* Root DELETE/slot replacement leave userdata behind until idle detach.
  * These and a dead PAGE at begin must not be read by direction management. */
 for(unsigned i=0;i<3;++i) for(unsigned which=0;which<3;++which) {
  struct p67_surface *s=bind_blur(i);scroll_event(s,9,LIST,PAGE,0);
  if(which==0) {
   uint32_t e[5];event(e,LIST,LIST,36,s->generation);p67_surface_event(e);
  }
  if(which==1) surface_root=FOREGROUND;
  if(which==2) object(PAGE)->live=0;
  surface_image=0;capture_fail=1; /* native begin needs no mock image swap */
  forbid_page_coords=1;scroll_event(s,9,LIST,PAGE,0);
  if(which!=2) scroll_event(s,12,LIST,PAGE,0);
  forbid_page_coords=0;
  assert(!s->session.page && !p67_blur_allowed_entries);
  p67_surface_detach(s);s->path[0]=0;
 }
 /* One native owner with different pages uses separate viewbits, never ownerbits. */
 reset();p67_path[0][0]=0;shared_mode=1;populated=0;hidden_root=0;
 surface_image=0;capture_fail=1;
 object(LIST)->clz=object(FOREGROUND)->clz=0x2cdba8b4u;
 object(LIST)->parent=PAGE;object(FOREGROUND)->parent=NEW_LIST;object(NEW_LIST)->parent=OWNER;
 object(OWNER)->count=2;
 object(OWNER)->d[0][0]=p67_surfaces[0].callback;object(OWNER)->d[0][1]=PAGE;
 object(OWNER)->d[1][0]=p67_surfaces[1].callback;object(OWNER)->d[1][1]=NEW_LIST;
 for(unsigned i=0;i<2;++i) {
  struct p67_surface *s=&p67_surfaces[i];
  memset(&s->owner,0,sizeof(*s)-offsetof(struct p67_surface,owner));strcpy(s->path,":blur");
  p67_surface_poll(s);assert(s->owner==OWNER);
  position(s,p67_surface_closed(s));selected=i;scroll_event(s,9,LIST,s->page,0);
 }
 assert(p67_blur_allowed_entries==6 && p67_blur_pending);
 uint32_t requested=p67_blur_request_serial;
 position(&p67_surfaces[0],0);selected=0;scroll_event(&p67_surfaces[0],9,OWNER,PAGE,0);
 assert(p67_blur_allowed_entries==4 && p67_blur_pending && p67_blur_armed);
 assert(p67_blur_request_serial==requested && p67_surfaces[1].session.page==NEW_LIST);
 p67_surface_detach(&p67_surfaces[0]);assert(p67_blur_allowed_entries==4);
 p67_surface_detach(&p67_surfaces[1]);assert(!p67_blur_allowed_entries && !p67_blur_pending);
 for(unsigned i=0;i<3;++i) p67_surfaces[i].path[0]=0;
 shared_mode=0;p67_blur_source=0;
}
/* A failed new capture may coexist with an immutable fallback. Open END makes
 * the native surface idle, but must leave its accepted entry budget pending. */
static struct p67_surface *idle_blur_pending(unsigned i) {
 struct p67_surface *s=bind_blur(i);
 populated=FOREGROUND;object(FOREGROUND)->clz=0x2cdba28cu;
 object(FOREGROUND)->parent=LIST;p67_surface_refresh_list(s);
 assert(s->list==FOREGROUND && object(FOREGROUND)->count==1);
 p67_blur_source=0x20001000u;
 scroll_event(s,9,LIST,PAGE,0);position(s,0);scroll_event(s,11,LIST,PAGE,0);
 assert(!s->moving && !s->swapped && p67_surface_idle(s));
 assert(s->session.page==PAGE && p67_blur_allowed_entries==(2u<<i));
 assert(p67_blur_pending && p67_blur_armed && p67_blur_source);
 return s;
}
static void check_idle_list_repair(void) {
 for(unsigned i=1;i<3;++i) {
  struct p67_surface *s=idle_blur_pending(i);
  uint32_t generation=s->generation,session_generation=s->session.generation;
  uint32_t request=p67_blur_request_serial,epoch=p67_blur_epoch;
  uintptr_t source=p67_blur_source;
  unsigned owner_count=object(OWNER)->count,root_count=object(LIST)->count;
  unsigned snapshots=captures,released=cleanup;
  uint32_t e[5];event(e,FOREGROUND,FOREGROUND,36,generation);p67_surface_event(e);
  object(FOREGROUND)->live=0;object(FOREGROUND)->count=0;populated=0;
  assert(!s->list && s->dirty);
  uint32_t list_epoch=s->epoch;
  /* Missing list, failed listener add, recreated list (same address), stable
   * repeat poll: none may detach the unchanged native page binding. */
  for(unsigned phase=0;phase<4;++phase) {
   if(phase==1) {
    object(FOREGROUND)->live=1;populated=FOREGROUND;fail_add=add_calls+1;
   }
   if(phase==2) fail_add=0;
   unsigned adds=add_calls;p67_surface_poll(s);
   assert(add_calls==adds+(phase==1 || phase==2));
   assert(s->owner==OWNER && s->page==PAGE && s->root==LIST);
   assert(s->generation==generation && s->epoch==list_epoch && !s->dirty);
   assert(s->session.generation==session_generation && s->session.page==PAGE);
   assert(p67_blur_allowed_entries==(2u<<i) && p67_blur_pending && p67_blur_armed);
   assert(p67_blur_request_serial==request && p67_blur_epoch==epoch && p67_blur_source==source);
   assert(object(OWNER)->count==owner_count && object(LIST)->count==root_count);
   uint32_t *d=p67_descriptor(OWNER,p67_scroll_fn(s));assert(d && d[1]==PAGE && !d[2]);
   assert(captures==snapshots && cleanup==released && !hidden_root && hidden_image);
   if(phase<2) assert(!s->list && !object(FOREGROUND)->count);
   else {
    assert(s->list==FOREGROUND && object(FOREGROUND)->count==1);
    d=p67_descriptor(FOREGROUND,p67_event_fn(s));assert(d && d[1]==generation && !d[2]);
   }
  }
  /* Root DELETE still invalidates the whole entry, even with a repaired list. */
  event(e,LIST,LIST,36,generation);p67_surface_event(e);
  object(LIST)->live=0;surface_root=0;p67_surface_poll(s);
  assert(!s->owner && !s->root && !s->list && !s->session.page);
  assert(!p67_blur_allowed_entries && !p67_blur_pending && !p67_blur_armed);
  assert(p67_blur_request_serial==request+1 && p67_blur_epoch==epoch+1);
  assert(p67_blur_source==source && !object(FOREGROUND)->count);
  s->path[0]=0;
 }
 /* Unknown class/parent/wrapper changes cannot masquerade as list repair. */
 for(unsigned i=1;i<3;++i) for(unsigned which=0;which<5;++which) {
  struct p67_surface *s=idle_blur_pending(i);
  uint32_t request=p67_blur_request_serial,epoch=p67_blur_epoch;
  uintptr_t source=p67_blur_source;
  if(which==0) object(LIST)->clz=0;
  if(which==1) object(LIST)->parent=FOREGROUND;
  if(which==2) object(PAGE)->parent=FOREGROUND;
  if(which==3) p67_descriptor(OWNER,p67_scroll_fn(s))[1]=FOREGROUND;
  if(which==4) p67_descriptor(OWNER,p67_scroll_fn(s))[2]=1;
  p67_surface_poll(s);
  assert(!s->owner && !s->root && !s->session.page);
  assert(!p67_blur_allowed_entries && !p67_blur_pending && !p67_blur_armed);
  assert(p67_blur_request_serial==request+1 && p67_blur_epoch==epoch+1);
  assert(p67_blur_source==source);s->path[0]=0;
 }
 p67_blur_source=0;populated=0;
}
/* A draw event proves its current target is alive, not retained pointers.
 * Prove ancestor identity without global walks and reject stale/detached pairs
 * before reading page bounds, including DELETE and same-address reuse. */
static void check_draw_binding(void) {
 for(unsigned i=0;i<3;++i) for(unsigned blur=0;blur<2;++blur)
 for(unsigned list_draw=0;list_draw<2;++list_draw) for(unsigned which=0;which<13;++which) {
  struct p67_surface *s=bind_blur(i);position(s,0);completed_capture();
  if(!blur) strcpy(s->path,"/data/static.bin");
  s->list=FOREGROUND;object(FOREGROUND)->parent=LIST;
  uint32_t obj=list_draw?FOREGROUND:LIST,e[5],gone[5];
  event(e,obj,obj,25,s->generation);
  if(which==1) --e[3];
  if(which==2) e[1]=OWNER;
  if(which==3) surface_root=NEW_LIST;
  if(which==4) object(LIST)->parent=NEW_LIST;
  if(which==5) object(PAGE)->parent=NEW_LIST;
  if(which==6) object(LIST)->parent=0;
  if(which==7 || which==8) {
   uint32_t deleted=which==7?LIST:OWNER;
   event(gone,deleted,deleted,36,s->generation);p67_surface_event(gone);
   object(deleted)->live=0;
   if(which==7) {
    /* A new object at the same address must not revive an old generation. */
    object(deleted)->live=1;object(deleted)->parent=PAGE;
   }
  }
  if(which==9) {
   /* The retained page is stale/dead, but the current root has a live parent.
    * Its mismatch must be detected before dereferencing the retained page. */
   s->page=NEW_LIST;object(NEW_LIST)->live=0;
  }
  if(which==10) {
   if(list_draw) object(FOREGROUND)->parent=FOREGROUND;
   else object(LIST)->parent=LIST;
  }
  if(which==11) s->page=0;
  if(which==12) {
   /* Page teardown detaches its root before freeing the ancestor. */
   object(LIST)->parent=0;object(PAGE)->live=0;
  }
  unsigned walks=tree_calls,parents=parent_calls,rendered=draws,adds=add_calls;
  uint32_t saved_clip[4];memcpy(saved_clip,layer_clip,sizeof(saved_clip));
  forbid_mutation=1;forbid_page_coords=which!=0;
  unsigned frames=which?1u:128u;
  for(unsigned n=0;n<frames;++n) p67_surface_event(e);
  assert(tree_calls==walks && add_calls==adds);
  assert(draws==rendered+(which?0u:frames));
  assert(!memcmp(saved_clip,layer_clip,sizeof(saved_clip)));
  if(!which) assert(parent_calls==parents+frames*(list_draw?3u:2u));
  else assert(parent_calls-parents<=17u);
  forbid_mutation=forbid_page_coords=0;
  s->owner=s->page=s->root=s->list=0;s->path[0]=0;
 }
 p67_blur_source=0;
}
static void check_draw_ancestry(void) {
 for(unsigned i=0;i<3;++i) for(unsigned blur=0;blur<2;++blur)
 for(unsigned which=0;which<5;++which) {
  struct p67_surface *s=bind_blur(i);position(s,0);completed_capture();
  if(!blur) strcpy(s->path,"/data/static.bin");
  s->list=FOREGROUND;object(FOREGROUND)->parent=LIST;
  if(which==0) {
   /* Allocation lifetime is distinct from registry membership: a native draw
    * of an off-screen tree still has a valid target and attached ancestors. */
   for(unsigned n=0;n<5;++n) objects[n].off_tree=1;
   assert(!p67_live(PAGE));
  }
  if(which==1) object(FOREGROUND)->parent=0;
  if(which==2) object(FOREGROUND)->parent=NEW_LIST;
  if(which>=3) {
   unsigned depth=which==3?14u:15u;
   for(unsigned n=0;n<depth;++n) {
    ancestors[n].id=600u+100u*n;ancestors[n].live=1;
    ancestors[n].parent=n+1u==depth?LIST:700u+100u*n;
   }
   object(FOREGROUND)->parent=ancestors[0].id;
  }
  unsigned walks=tree_calls,parents=parent_calls,rendered=draws;
  uint32_t e[5],saved[4];event(e,FOREGROUND,FOREGROUND,25,s->generation);
  memcpy(saved,layer_clip,sizeof(saved));
  unsigned accepted=which==0 || which==3;
  forbid_mutation=1;forbid_page_coords=!accepted;p67_surface_event(e);
  forbid_mutation=forbid_page_coords=0;
  assert(tree_calls==walks && draws==rendered+accepted);
  assert(!memcmp(saved,layer_clip,sizeof(saved)));
  if(which==0) assert(parent_calls==parents+3u);
  if(which==3) assert(parent_calls==parents+17u);
  if(which==4) assert(parent_calls==parents+16u);
  s->owner=s->page=s->root=s->list=0;s->path[0]=0;
 }
 p67_blur_source=0;
}
/* Event12 stays mutation-free even when a list appears or is recreated.
 * Only the UI-owner poll repairs its subscription; root draws remain valid. */
static void check_scroll_list_repair(void) {
 for(unsigned i=1;i<3;++i) for(unsigned blur=0;blur<2;++blur) {
  struct p67_surface *s=bind_blur(i);position(s,i==1?-100:100);completed_capture();
  if(!blur) strcpy(s->path,"/data/static.bin");
  s->moving=s->swapped=1;
  populated=FOREGROUND;object(FOREGROUND)->clz=0x2cdba28cu;
  object(FOREGROUND)->parent=LIST;
  uint32_t generation=s->generation,e[5];
  for(unsigned recreated=0;recreated<2;++recreated) {
   unsigned walks=tree_calls,parents=parent_calls,adds=add_calls;
   forbid_mutation=1;
   scroll_event(s,12,LIST,PAGE,0);
   forbid_mutation=0;
   assert(tree_calls==walks && parent_calls==parents && add_calls==adds && !s->list);
   p67_surface_poll(s);
   assert(s->list==FOREGROUND && s->generation==generation);
   assert(object(FOREGROUND)->count==1 && !hidden_root && hidden_image);
   position(s,0);walks=tree_calls;parents=parent_calls;
   unsigned rendered=draws;
   event(e,FOREGROUND,FOREGROUND,25,generation);
   forbid_mutation=1;p67_surface_event(e);forbid_mutation=0;
   assert(tree_calls==walks && parent_calls==parents+3u && draws==rendered+1u);
   position(s,i==1?-100:100);
   /* List-only DELETE preserves the page token, but old list draws are gated. */
   event(e,FOREGROUND,FOREGROUND,36,generation);p67_surface_event(e);
   object(FOREGROUND)->live=0;object(FOREGROUND)->count=0;populated=0;
   walks=tree_calls;parents=parent_calls;
   event(e,FOREGROUND,FOREGROUND,25,generation);
   forbid_page_coords=forbid_mutation=1;p67_surface_event(e);
   forbid_page_coords=forbid_mutation=0;
   assert(tree_calls==walks && parent_calls==parents && !s->list);
   assert(s->generation==generation && s->moving && s->swapped);
   object(FOREGROUND)->live=1;populated=FOREGROUND;
   /* Reusing the same address cannot enable draws before listener repair. */
   walks=tree_calls;parents=parent_calls;rendered=draws;
   forbid_page_coords=forbid_mutation=1;p67_surface_event(e);
   forbid_page_coords=forbid_mutation=0;
   assert(tree_calls==walks && parent_calls==parents && draws==rendered && !s->list);
  }
  p67_surface_detach(s);s->path[0]=0;
 }
 p67_blur_source=0;populated=0;
}
int main(void) {
 check_draw_ancestry();
 check_scroll_list_repair();
 check_draw_binding();
 check_terminal_ends();
 check_blur_sessions();
 check_idle_list_repair();
 uint32_t e[5];
 for(unsigned i=0;i<3;++i) {
  reset();selected=i;first_enter=cache_reuses=captures=0;populated=0;delete_on_flag=0;p67_path[0][0]=0;
  struct p67_surface *s=&p67_surfaces[i];
  s->owner=s->page=s->root=s->list=s->moving=s->swapped=s->busy=0;
  surface_root=LIST;surface_image=NEW_LIST;hidden_root=0;hidden_image=1;capture_fail=0;cleanup=0;
  object(LIST)->clz=0x2cdba8b4u;object(NEW_LIST)->clz=RH_FW_IMAGE_CLASS;
  object(LIST)->parent=object(NEW_LIST)->parent=PAGE;
  object(OWNER)->d[0][0]=s->callback;strcpy(s->path,"/data/static.bin");
  fail_add=2;p67_surface_poll(s);assert(!s->owner && object(OWNER)->count==1);
  fail_add=0;p67_surface_poll(s);assert(s->owner==OWNER);
  event(e,OWNER,LIST,9,PAGE);p67_surface_scroll_for(s,e);
  assert(forwards[9]==1 && s->moving && s->swapped && !hidden_root && hidden_image);
  if(i==2) assert(first_enter==1 && cache_reuses==1 && !captures);
  assert(object(OWNER)->d[0][1]==PAGE && !object(OWNER)->d[0][2]);
  check_non_gestures(s);draws=0;
  /* Intermediate native end is not terminal and must stay live/busy. */
  if(i==1) object(PAGE)->y=(uint32_t)-100;
  else object(PAGE)->bounds[0]=i==0?(uint32_t)-100:100;
  event(e,OWNER,LIST,11,PAGE);p67_surface_scroll_for(s,e);
  assert(!cleanup && s->moving && s->swapped && !hidden_root && hidden_image);
  struct rh_backgrounds bg;memset(&bg,0,sizeof(bg));
  assert(rh_platform_backgrounds_commit(&bg)==-2145);
  /* Reversal begin preserves native bookkeeping then terminal end restores. */
  if(i==1) object(PAGE)->y=0;else object(PAGE)->bounds[0]=0;
  event(e,OWNER,LIST,9,PAGE);p67_surface_scroll_for(s,e);assert(s->swapped);
  if(i>0) {
   populated=FOREGROUND;object(FOREGROUND)->clz=0x2cdba28cu;object(FOREGROUND)->parent=LIST;
   p67_surface_poll(s);assert(s->list==FOREGROUND && object(FOREGROUND)->count==1);
   event(e,FOREGROUND,FOREGROUND,36,s->generation);p67_surface_event(e);
   object(FOREGROUND)->live=0;populated=0;
   assert(s->moving && !s->list);
   object(FOREGROUND)->live=1;object(FOREGROUND)->count=0;populated=FOREGROUND;
   p67_surface_poll(s);assert(s->moving && s->list==FOREGROUND);
  }
  check_steady_scroll(s);
  object(PAGE)->y=120;object(PAGE)->bounds[3]=599;
  layer_clip[1]=100;layer_clip[3]=600;expected_clip[1]=120;expected_clip[3]=479;
  uint32_t saved[4];memcpy(saved,layer_clip,sizeof(saved));
  event(e,LIST,LIST,25,s->generation);p67_surface_event(e);assert(draws==1);
  assert(!memcmp(saved,layer_clip,sizeof(saved)));
  object(PAGE)->y=480;p67_surface_event(e);assert(draws==1);
  object(PAGE)->y=0;
  event(e,OWNER,LIST,11,PAGE);p67_surface_scroll_for(s,e);
  assert(cleanup==1 && !s->moving && !s->swapped && !hidden_root);
  surface_image=0;capture_fail=1;
  event(e,OWNER,LIST,9,PAGE);p67_surface_scroll_for(s,e);assert(!s->swapped);
  event(e,OWNER,LIST,11,PAGE);p67_surface_scroll_for(s,e);
  /* Direct owner DELETE restores native wrapper immediately; survivor cleanup
   * is deferred without walking the deleting subtree. */
  event(e,OWNER,OWNER,36,s->generation);p67_surface_event(e);
  assert(!s->owner && s->dirty && object(OWNER)->d[0][0]==s->callback);
  object(OWNER)->live=0;p67_surface_detach(s);
  assert(!s->owner && !object(LIST)->count);object(OWNER)->live=1;
  assert(!object(FOREGROUND)->count);
  assert(object(OWNER)->d[0][0]==s->callback);
  p67_surface_poll(s);assert(s->owner);
  capture_fail=0;surface_image=NEW_LIST;hidden_image=1;
  delete_on_flag=1;
  event(e,OWNER,LIST,9,PAGE);p67_surface_scroll_for(s,e);
  assert(!s->root && !object(LIST)->live && !s->swapped);
  p67_surface_detach(s);
  /* Native-visibility restoration itself may delete the live root. */
  object(LIST)->live=1;object(LIST)->count=0;surface_root=LIST;hidden_root=0;hidden_image=1;
  s->root=LIST;s->page=PAGE;s->swapped=1;delete_on_remove=1;
  p67_surface_native(s);assert(!s->root && !hidden_root && s->swapped);
  p67_surface_detach(s);
  s->path[0]=0;
 }
 /* Two page bindings on one owner have unique wrapper/event identities. */
 reset();p67_path[0][0]=0;shared_mode=1;populated=0;hidden_root=0;
 object(LIST)->clz=object(FOREGROUND)->clz=0x2cdba8b4u;
 object(LIST)->parent=PAGE;object(FOREGROUND)->parent=NEW_LIST;object(NEW_LIST)->parent=OWNER;
 object(OWNER)->count=2;
 object(OWNER)->d[0][0]=p67_surfaces[0].callback;object(OWNER)->d[0][1]=PAGE;
 object(OWNER)->d[1][0]=p67_surfaces[1].callback;object(OWNER)->d[1][1]=NEW_LIST;
 for(unsigned i=0;i<2;++i) {strcpy(p67_surfaces[i].path,"/data/static.bin");p67_surfaces[i].path_index=0;p67_surface_poll(&p67_surfaces[i]);assert(p67_surfaces[i].owner==OWNER);}
 assert(p67_surfaces[0].generation!=p67_surfaces[1].generation);
 assert(object(OWNER)->count==4);
 p67_surface_detach(&p67_surfaces[0]);assert(object(OWNER)->count==3);
 assert(p67_descriptor(OWNER,p67_scroll_fn(&p67_surfaces[1])));
 p67_surface_detach(&p67_surfaces[1]);assert(object(OWNER)->count==2);
 assert(p67_descriptor(OWNER,p67_surfaces[0].callback) && p67_descriptor(OWNER,p67_surfaces[1].callback));
 /* Same owner AND same page: wrapper identity, not a userdata loop,
  * determines which native callback is forwarded. */
 p67_surfaces[0].owner=p67_surfaces[2].owner=OWNER;
 p67_surfaces[0].page=p67_surfaces[2].page=PAGE;
 p67_surfaces[0].root=p67_surfaces[2].root=LIST;
 p67_surfaces[0].busy=p67_surfaces[2].busy=0;
 native_cc_calls=native_notify_calls=native_widget_calls=0;
 event(e,OWNER,LIST,20,PAGE);p67_scroll_widget(e);
 assert(native_widget_calls==1 && !native_cc_calls && !native_notify_calls);
 p67_scroll_cc(e);assert(native_cc_calls==1 && native_widget_calls==1);
 event(e,LIST,LIST,20,PAGE);p67_scroll_widget(e);assert(native_widget_calls==1);
 /* Deferred blur configuration accepts mixed/static/system and all blur.
  * No literal token reaches image-info/draw; missing generation stays native. */
 reset();shared_mode=0;surface_root=surface_image=populated=0;
 for(unsigned i=0;i<3;++i) memset(&p67_surfaces[i].owner,0,
     sizeof(p67_surfaces[i])-offsetof(struct p67_surface,owner));
 struct rh_backgrounds blur_bg;memset(&blur_bg,0,sizeof(blur_bg));
 strcpy(blur_bg.launcher,":blur");strcpy(blur_bg.control_center,"/data/static.bin");
 strcpy(blur_bg.widgets,":blur");
 assert(!rh_platform_backgrounds_stage(&blur_bg));
 assert(!rh_platform_backgrounds_commit(&blur_bg));
 assert(!p67_source(p67_path[p67_path_index]));
 assert(!p67_source(p67_surface_path(&p67_surfaces[2])));
 strcpy(blur_bg.control_center,":blur");strcpy(blur_bg.notifications,":blur");
 assert(!rh_platform_backgrounds_stage(&blur_bg));
 assert(!rh_platform_backgrounds_commit(&blur_bg));
 assert(rh_background_is_blur(p67_surface_path(&p67_surfaces[1])));
 /* Launcher consumes exactly the same immutable source as all three pages. */
 draws=0;p67_blur_source=0;
 event(e,LIST,LIST,25,p67_generation);p67_event(e);assert(!draws);
 p67_blur_source=0x20001000u;
 event(e,LIST,LIST,25,p67_generation);p67_event(e);
 assert(draws==1 && last_draw_source==p67_blur_source);
 for(unsigned i=0;i<3;++i) {
  reset();selected=i;p67_path[0][0]=0;populated=0;
  struct p67_surface *s=&p67_surfaces[i];
  s->owner=OWNER;s->page=PAGE;s->root=LIST;s->list=0;s->busy=s->moving=s->swapped=0;
  s->generation=900+i;s->path_index=0;strcpy(s->path,":blur");
  surface_root=LIST;surface_image=NEW_LIST;hidden_root=0;hidden_image=1;
  object(LIST)->parent=object(NEW_LIST)->parent=PAGE;
  object(LIST)->clz=0x2cdba8b4u;object(NEW_LIST)->clz=RH_FW_IMAGE_CLASS;
  p67_blur_source=0;
  event(e,LIST,LIST,25,s->generation);p67_surface_event(e);assert(!draws);
  p67_blur_source=0x20001000u;
  unsigned submitted=image_draws,initialized=image_inits;
  uint32_t copies=p67_blur_copies;
  event(e,LIST,LIST,25,s->generation);p67_surface_event(e);
  assert(draws==1 && last_draw_source==p67_blur_source);
  assert(image_draws==submitted+1 && image_inits==initialized+1);
  /* Full and partial reveal use the same screen-origin 4x image transform. */
  object(PAGE)->y=120;object(PAGE)->bounds[3]=599;
  layer_clip[0]=(uint32_t)-20;layer_clip[1]=100;layer_clip[2]=360;layer_clip[3]=600;
  expected_clip[0]=0;expected_clip[1]=120;expected_clip[2]=335;expected_clip[3]=479;
  uint32_t saved_clip[4];memcpy(saved_clip,layer_clip,sizeof(saved_clip));
  forbid_mutation=1;
  p67_surface_event(e);
  assert(draws==2 && image_draws==submitted+2 && !memcmp(saved_clip,layer_clip,sizeof(saved_clip)));
  assert(last_image_descriptor[7]==p67_blur_source && p67_blur_copies==copies);
  object(PAGE)->y=480;p67_surface_event(e);assert(draws==2);
  assert(!memcmp(saved_clip,layer_clip,sizeof(saved_clip)));
  object(PAGE)->y=120;
  event(e,LIST,LIST,25,s->generation-1u);p67_surface_event(e);assert(draws==2);
  event(e,LIST,FOREGROUND,25,s->generation);p67_surface_event(e);assert(draws==2);
  event(e,LIST,LIST,24,s->generation);p67_surface_event(e);assert(draws==2);
  event(e,LIST,LIST,25,s->generation);s->busy=1;p67_surface_event(e);assert(draws==2);s->busy=0;
  if(i>0) {
   s->list=FOREGROUND;object(FOREGROUND)->parent=LIST;
   /* A scrolled content list must not move the background or widen its viewport. */
   object(FOREGROUND)->y=(uint32_t)-400;object(FOREGROUND)->bounds[3]=1000;
   event(e,FOREGROUND,FOREGROUND,25,s->generation);p67_surface_event(e);
   assert(draws==3 && image_draws==submitted+3 && !memcmp(saved_clip,layer_clip,sizeof(saved_clip)));
   assert(last_image_descriptor[12]==1024 && !last_image_descriptor[17]);
   s->list=0;
  }
  forbid_mutation=0;
  /* Blur steady scroll must not mutate listeners, capture or visibility. */
  object(OWNER)->d[0][0]=p67_scroll_fn(s);
  s->moving=1;check_steady_scroll(s);s->moving=0;
  p67_blur_source=0;s->owner=s->page=s->root=0;s->path[0]=0;
 }

 {
  /* Control Center edit button transparency hook. */
  struct p67_surface *cc = &p67_surfaces[0];
  memset(cc, 0, sizeof(*cc));
  cc->root_slot = 0x2010fe50u;
  cc->callback = 0x0c5036c5u;
  strcpy(cc->path, "themes/test/cc.bin");
  surface_root = LIST;
  object(LIST)->live = 1;
  object(LIST)->parent = PAGE;
  object(PAGE)->live = 1;
  object(PAGE)->parent = OWNER;
  object(OWNER)->live = 1;
  object(OWNER)->d[0][0] = cc->callback;
  object(OWNER)->d[0][1] = PAGE;
  object(OWNER)->d[0][2] = 0;
  object(OWNER)->count = 1;
  p67_surface_poll(cc);
  assert(p67_surface_binding_valid(cc));

  edit_container_slot = FOREGROUND;
  object(FOREGROUND)->live = 1;
  edit_opa_calls = 0;
  rh_platform_backgrounds_poll();
  assert(edit_opa_calls == 1);
  assert(last_edit_opa_obj == FOREGROUND && last_edit_opa_val == 0u);

  /* Idempotent: poll does not repeatedly call style setter. */
  rh_platform_backgrounds_poll();
  assert(edit_opa_calls == 1);

  /* Detaching restores opaque background. */
  p67_surface_detach(cc);
  assert(edit_opa_calls == 2);
  assert(last_edit_opa_obj == FOREGROUND && last_edit_opa_val == 255u);
  edit_container_slot = 0;
 }

 puts("p67 three native-owned live surfaces and deferred blur modes passed");return 0;
}
