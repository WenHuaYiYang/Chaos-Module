/* Active, owned static blur. Draw callers only read the immutable source. */
#include "resource_hook_p67_blur.h"
#ifndef P67_BLUR_AVAILABLE
#define P67_BLUR_AVAILABLE 0
#endif
#ifndef P67_BLUR_COPY
#define P67_BLUR_COPY(plane, identity) 0
#endif
#ifndef P67_BLUR_PUBLISH
#define P67_BLUR_PUBLISH(plane, epoch) 0
#endif
#ifndef P67_BLUR_RETIRE
#define P67_BLUR_RETIRE(source) ((void)(source))
#endif
#ifndef P67_BLUR_MAINTAIN
#define P67_BLUR_MAINTAIN() ((void)0)
#endif
/* Pure owned-resource state only (input or retired output slots). This must
 * not query the GPU/native registry or count the live immutable output. */
#ifndef P67_BLUR_CLEANUP_PENDING
#define P67_BLUR_CLEANUP_PENDING() 0
#endif
#ifndef P67_BLUR_OWNER_SAFE
#define P67_BLUR_OWNER_SAFE() 1
#endif
#ifndef P67_BLUR_WATCHFACE
#define P67_BLUR_WATCHFACE() 0u
#endif
#ifndef P67_BLUR_DISPLAY
#ifdef RH_FW_DISPLAY_SLOT
#define P67_BLUR_DISPLAY() load32(RH_FW_DISPLAY_SLOT)
#else
#define P67_BLUR_DISPLAY() 0u
#endif
#endif
/* Display hooks must use the validated DISPLAY API, never LVGL object APIs.
 * Unconfigured hooks fail closed until the firmware provider supplies them. */
#ifndef P67_BLUR_DISPLAY_ADD
#define P67_BLUR_DISPLAY_ADD(display, callback, generation) \
 ((void)(display),(void)(callback),(void)(generation),(uint32_t *)0)
#endif
#ifndef P67_BLUR_DISPLAY_REMOVE
#define P67_BLUR_DISPLAY_REMOVE(display, callback) ((void)(display),(void)(callback),0)
#endif
#ifndef P67_BLUR_DISPLAY_COUNT
#define P67_BLUR_DISPLAY_COUNT(display) ((void)(display),65u)
#endif
#ifndef P67_BLUR_DISPLAY_GET
#define P67_BLUR_DISPLAY_GET(display, index) ((void)(display),(void)(index),(uint32_t *)0)
#endif
#ifndef P67_BLUR_WAKE
#define P67_BLUR_WAKE() rh_platform_request_full_redraw()
#endif
#ifndef P67_BLUR_VISIBLE
#define P67_BLUR_VISIBLE() p67_blur_demand
#endif
#ifndef P67_BLUR_DARKEN_PERCENT
#define P67_BLUR_DARKEN_PERCENT 100u
#endif
static uint16_t p67_blur_plane[P67_BLUR_PIXELS],p67_blur_work[P67_BLUR_PIXELS];
static uintptr_t p67_blur_source,p67_blur_retired;
static uint32_t p67_blur_epoch,p67_blur_config_epoch;
static uint32_t p67_blur_root,p67_blur_root_generation,p67_blur_root_serial;
static uint32_t p67_blur_display,p67_blur_display_generation,p67_blur_display_serial;
static unsigned p67_blur_root_registered,p67_blur_busy,p67_blur_polling;
static unsigned p67_blur_demand,p67_blur_visible,p67_blur_pending,p67_blur_wake;
static uint32_t p67_blur_request_serial;
/* One render attempt and one automatic wake per request/UI poll. Pending is
 * durable; armed is a consumable budget, not permission to retry every frame. */
static unsigned p67_blur_armed,p67_blur_request_wake,p67_blur_cleanup_armed;
/* Latched at attempts, never recomputed by diagnostics or ordinary frames:
 * 0 idle/ready, 1 unsupported, 2 disabled, 3 owner unsafe, 4 display invalid,
 * 5 root registration, 6 identity, 7 COPY, 8 publish, 9 stale epoch, 10 success. */
static uint32_t p67_blur_core_gate=P67_BLUR_AVAILABLE?0u:1u;
static int p67_blur_cleanup_pending(void) {
 return p67_blur_retired || P67_BLUR_CLEANUP_PENDING();
}
static uint32_t p67_blur_frame52,p67_blur_attempts,p67_blur_copies;
static uint32_t p67_blur_publishes,p67_blur_wakes,p67_blur_draws;
static inline void p67_blur_draw_seen(void) { ++p67_blur_draws; }
static void p67_blur_request(void) {
 ++p67_blur_request_serial; p67_blur_pending=1;
 p67_blur_armed=1; p67_blur_request_wake=1;
}
/* Before gesture direction is known, retain the existing initialization/config
 * prewarm. Once scoped, only an accepted closed->page entry permits capture. */
static unsigned p67_blur_entry_scoped,p67_blur_allowed_entries;
struct p67_blur_session { uint32_t generation,page; };
static uint32_t p67_blur_begin_skips;
static inline int p67_blur_capture_allowed(void) {
 return !p67_blur_entry_scoped || p67_blur_allowed_entries;
}
static inline void p67_blur_cancel_budget(void) {
 /* Invalidate in-flight publication, but keep the immutable published source. */
 ++p67_blur_epoch; ++p67_blur_request_serial;
 p67_blur_pending=0; p67_blur_armed=0; p67_blur_request_wake=0;
}
static inline void p67_blur_session_clear(struct p67_blur_session *s,unsigned bit) {
 unsigned previous=p67_blur_allowed_entries;
 /* Explicit words preserve zero-import ARM (__aeabi_memclr4 is unavailable). */
 s->generation=0; s->page=0;
 p67_blur_allowed_entries &= ~bit;
 if(previous && !p67_blur_allowed_entries && p67_blur_entry_scoped)
  p67_blur_cancel_budget();
}
/* Called only for a verified native binding, with its PAGE coordinate. No
 * target, input, or animation parameter is needed: both kinds of 9 precede
 * the first coordinate update. Partial begins cannot establish provenance.
 * exit_intent is a pure current-owner/original-target equality, not retained. */
static inline void p67_blur_session_event(struct p67_blur_session *s,unsigned bit,
 uint32_t generation,uint32_t page,unsigned code,int32_t position,int32_t closed,
 unsigned exit_intent) {
 unsigned scoped=p67_blur_entry_scoped,previous=p67_blur_allowed_entries;
 if(code!=9 && code!=11 && code!=12) return;
 if(code==9 && (position!=0 || exit_intent)) p67_blur_entry_scoped=1;
 if(s->generation!=generation || s->page!=page)
  p67_blur_session_clear(s,bit);
 if(code!=9) {
  if(position==closed) p67_blur_session_clear(s,bit);
  return;
 }
 if(position==0 && !exit_intent) {
  ++p67_blur_begin_skips; return; /* Bubbled content scroll, not a page exit. */
 }
 if(position==closed && !(p67_blur_allowed_entries & bit)) {
  s->generation=generation; s->page=page;
  p67_blur_allowed_entries |= bit;
  p67_blur_request();
  return; /* First 9 at closed is the entry, not its terminal cancellation. */
 }
 ++p67_blur_begin_skips; /* exit, partial/snap/reversal, or repeated begin */
 if(position==0) p67_blur_session_clear(s,bit);
 if(!scoped && !previous && !p67_blur_allowed_entries) p67_blur_cancel_budget();
}
static uintptr_t p67_source(const char *path) {
 return !path || !path[0]?0:rh_background_is_blur(path)?p67_blur_source:(uintptr_t)path;
}
/* Logical retirement only: DELETE/reset never fence or free. One published
 * output and one deferred retirement bound storage even during reentrancy. */
static void p67_blur_drop(void) {
 ++p67_blur_epoch;
 if(p67_blur_source) { p67_blur_retired=p67_blur_source; p67_blur_source=0; p67_blur_wake=1; }
 /* Lifecycle invalidation waits for the next UI poll to retry. It must not
  * replenish an attempt consumed before a failing root/owner gate. */
 ++p67_blur_request_serial; p67_blur_pending=1;
 if(!p67_blur_capture_allowed()) { p67_blur_armed=0; p67_blur_request_wake=0; }
}
static void p67_blur_reset(void) {
 /* No native reads here (reset may run inside DELETE or publication). The
  * next safe START/watch checks root identity; same-root immutable pixels stay. */
 ++p67_blur_config_epoch; ++p67_blur_epoch;
 p67_blur_entry_scoped=0; p67_blur_request();
}
static void p67_blur_release(void) {
 uintptr_t old=p67_blur_retired;
 p67_blur_retired=0;
 if(old) P67_BLUR_RETIRE(old);
}
static void p67_blur_root_event(void *event) {
    uint32_t object=*(uint32_t *)event,target=*((uint32_t *)event+1);
    unsigned code=*(uint16_t *)((char *)event+8)&0x7fffu;
    if(code!=36 || object!=target || object!=p67_blur_root ||
       *((uint32_t *)event+3)!=p67_blur_root_generation) return;
    p67_blur_root=0; p67_blur_root_registered=0;
    p67_blur_root_generation=++p67_blur_root_serial;
    p67_blur_drop();

}
static void p67_blur_root_unbind(void) {
    uint32_t root=p67_blur_root,generation=p67_blur_root_generation,*d;
    p67_blur_root=0; p67_blur_root_registered=0;
    p67_blur_root_generation=++p67_blur_root_serial;
    if(p67_live(root)) {
        d=p67_descriptor(root,(uint32_t)(uintptr_t)p67_blur_root_event);
        if(d && d[1]==generation && d[2]==36)
            ((int (*)(uint32_t,uint32_t))(uintptr_t)P67_NATIVE(0x002bf811u))
                (root,(uint32_t)(uintptr_t)p67_blur_root_event);
    }
}
static int p67_blur_root_callback_absent(uint32_t root) {
    unsigned i;
    uint32_t count=((uint32_t (*)(uint32_t))(uintptr_t)P67_NATIVE(0x002bf7d5u))(root);
    if(count>64) return 0;
    for(i=0;i<count;++i) {
        uint32_t *d=((uint32_t *(*)(uint32_t,uint32_t))(uintptr_t)
            P67_NATIVE(0x002bf7e9u))(root,i);
        if(!d || d[0]==(uint32_t)(uintptr_t)p67_blur_root_event) return 0;
    }
    return 1; /* Ambiguous/stale records are not permission to add another. */
}
static void p67_blur_root_watch(void) {
    uint32_t root=P67_BLUR_WATCHFACE(),generation,*d,*added;
    if(root==p67_blur_root && p67_blur_root_registered) {
        d=p67_descriptor(root,(uint32_t)(uintptr_t)p67_blur_root_event);
        if(d && d[1]==p67_blur_root_generation && d[2]==36) return;
    }
    if(p67_blur_root) {
        p67_blur_root_unbind(); p67_blur_drop();
    }
    if(!root || !p67_live(root) || root!=P67_BLUR_WATCHFACE() ||
       !p67_blur_root_callback_absent(root)) return;
    generation=++p67_blur_root_serial;
    /* Establish a tentative identity before native registration so synchronous
     * DELETE/config callbacks invalidate it rather than resurrecting the root. */
    p67_blur_root=root; p67_blur_root_generation=generation;
    added=((uint32_t *(*)(uint32_t,uint32_t,uint32_t,uint32_t))(uintptr_t)
        P67_NATIVE(0x0c5883f1u))(root,(uint32_t)(uintptr_t)p67_blur_root_event,36,generation);
    if(added && root==p67_blur_root && generation==p67_blur_root_generation &&
       root==P67_BLUR_WATCHFACE() && p67_live(root)) {
        d=p67_descriptor(root,(uint32_t)(uintptr_t)p67_blur_root_event);
        if(d && d[1]==generation && d[2]==36) { p67_blur_root_registered=1; return; }
    }
    if(root==p67_blur_root && generation==p67_blur_root_generation) p67_blur_root_unbind();
}

#ifndef P67_BLUR_DISPLAY_LIVE
#ifdef RH_FW_FR_DISPLAY_LIST
/* Registry links are not permission to dereference arbitrary memory. */
#ifndef P67_BLUR_DISPLAY_RAM
#define P67_BLUR_DISPLAY_RAM(address, size) \
 (((address)>=0x3c271400u && (address)<0x3dfc0000u && (size)<=0x3dfc0000u-(address)) || \
  ((address)>=0x200765c0u && (address)<0x2015fec0u && (size)<=0x2015fec0u-(address)))
#endif
static int p67_blur_display_live(uint32_t display) {
 uint32_t p,prev=0,count=0;
 unsigned found=0;
 if(!display || load32(RH_FW_FR_DISPLAY_LIST)!=792u) return 0;
 for(p=load32(RH_FW_FR_DISPLAY_LIST+4u);p;p=load32(p+796u)) {
  if(++count>4u || (p&3u) || !P67_BLUR_DISPLAY_RAM(p,800u) || load32(p+792u)!=prev) return 0;
  if(p==display) found=1;
  prev=p;
 }
 return found && load32(RH_FW_FR_DISPLAY_LIST+8u)==prev;
}
#define P67_BLUR_DISPLAY_LIVE(display) p67_blur_display_live(display)
#else
#define P67_BLUR_DISPLAY_LIVE(display) ((display)==P67_BLUR_DISPLAY())
#endif
#endif
static void p67_blur_display_event(void *event);
/* Display is not an LVGL object: validate its event records directly. */
static uint32_t *p67_blur_display_record(uint32_t display) {
 unsigned i,count;
 uint32_t *found=0;
 count=P67_BLUR_DISPLAY_COUNT(display);
 if(count>64) return 0;
 for(i=0;i<count;++i) {
  uint32_t *d=P67_BLUR_DISPLAY_GET(display,i);
  if(!d) return 0;
  if(d[0]==(uint32_t)(uintptr_t)p67_blur_display_event) { if(found) return 0; found=d; }
 }
 return found;
}
static int p67_blur_display_valid(void) {
 uint32_t *d;
 if(!p67_blur_display || p67_blur_display!=P67_BLUR_DISPLAY() || !P67_BLUR_DISPLAY_LIVE(p67_blur_display)) return 0;
 d=p67_blur_display_record(p67_blur_display);
 return d && d[1]==p67_blur_display_generation && d[2]==0x8000u;
}
static void p67_blur_display_unbind(void) {
 uint32_t display=p67_blur_display;
 uint32_t *d=P67_BLUR_DISPLAY_LIVE(display)?p67_blur_display_record(display):0;
 unsigned valid=d && d[1]==p67_blur_display_generation && d[2]==0x8000u;
 p67_blur_display=0; p67_blur_display_generation=++p67_blur_display_serial;
 if(valid) (void)P67_BLUR_DISPLAY_REMOVE(display,p67_blur_display_event);
}

static void p67_blur_display_event(void *event) {
 uint32_t display=*(uint32_t *)event;
 unsigned code=*(uint16_t *)((char *)event+8)&0x7fffu;
 uint32_t epoch,generation,request,display_generation,identity=0;
 uintptr_t next,old;
 unsigned visible;
 if(display!=p67_blur_display || *((uint32_t *)event+3)!=p67_blur_display_generation) return;
 if(code==36) { p67_blur_display=0; p67_blur_display_generation=++p67_blur_display_serial; p67_blur_drop(); return; }
 if(p67_blur_busy || p67_blur_polling) return;
 if(code==53) {
  epoch=p67_blur_epoch; generation=p67_blur_display_generation;
  request=p67_blur_request_serial;
  if(p67_blur_capture_allowed() &&
     (p67_blur_wake || (p67_blur_pending && p67_blur_armed && p67_blur_request_wake &&
                       p67_blur_demand && p67_blur_visible))) {
   int status;
   /* Invalidation may synchronously dispatch READY, reset, or DELETE. */
   p67_blur_busy=1;
   /* Consume before a native call, including failure/reentrant dispatch. */
   p67_blur_wake=0; p67_blur_request_wake=0;
   if(!p67_blur_display_valid()) {
    p67_wake_pending=1; p67_blur_busy=0; return;
   }
   ++p67_blur_wakes;
   status=P67_BLUR_WAKE(); /* Platform ABI: zero is success. */
   if(status!=0 || epoch!=p67_blur_epoch || generation!=p67_blur_display_generation ||
      request!=p67_blur_request_serial || !p67_blur_display_valid())
    p67_wake_pending=1; /* Never clear a reentrant lifecycle's wake. */
   p67_blur_busy=0;
  }
  return;
 }
 if(code!=52) return;
 ++p67_blur_frame52;
 if(!P67_BLUR_AVAILABLE) {
  if(p67_blur_armed || p67_blur_cleanup_armed) {
   p67_blur_armed=0; p67_blur_request_wake=0; p67_blur_cleanup_armed=0;
   ++p67_blur_attempts; p67_blur_core_gate=1;
  }
  return;
 }
 if(!p67_blur_demand) {
  if(!p67_blur_cleanup_armed || !p67_blur_cleanup_pending()) return;
  /* A disabled blur still owns resources until a safe fence completes. This
   * budget never authorizes COPY/filter/publish and READY cannot replenish it. */
  p67_blur_cleanup_armed=0; ++p67_blur_attempts; p67_blur_core_gate=2;
  epoch=p67_blur_epoch; display_generation=p67_blur_display_generation;
  if(!P67_BLUR_OWNER_SAFE()) { p67_blur_core_gate=3; return; }
  if(!p67_blur_display_valid()) { p67_blur_core_gate=4; return; }
  p67_blur_busy=1;
  p67_blur_release(); P67_BLUR_MAINTAIN();
  if(epoch!=p67_blur_epoch || display_generation!=p67_blur_display_generation)
   p67_blur_core_gate=9;
  else if(!p67_blur_display_valid()) p67_blur_core_gate=4;
  else if(!p67_blur_cleanup_pending()) p67_blur_core_gate=10;
  p67_blur_busy=0;
  return;
 }
 if(!p67_blur_capture_allowed()) return;
 visible=!!P67_BLUR_VISIBLE();
 if(visible && !p67_blur_visible && !p67_blur_entry_scoped && !p67_blur_pending)
  p67_blur_request();
 p67_blur_visible=visible;
 if(!visible || !p67_blur_pending || !p67_blur_armed) return;
 p67_blur_armed=0; p67_blur_request_wake=0; ++p67_blur_attempts;
 p67_blur_core_gate=0;
 epoch=p67_blur_config_epoch;
 if(!P67_BLUR_OWNER_SAFE()) { p67_blur_core_gate=3; return; }
 if(!p67_blur_display_valid()) { p67_blur_core_gate=4; return; }
 p67_blur_polling=1;
 p67_blur_root_watch();
 p67_blur_polling=0;
 if(epoch!=p67_blur_config_epoch) { p67_blur_core_gate=9; return; }
 if(!p67_blur_root_registered) { p67_blur_core_gate=5; return; }
 if(!p67_blur_display_valid()) { p67_blur_core_gate=4; return; }
 if(P67_BLUR_WATCHFACE()!=p67_blur_root) { p67_blur_core_gate=6; p67_blur_drop(); return; }
 p67_blur_busy=1;
 epoch=p67_blur_epoch; generation=p67_blur_root_generation; request=p67_blur_request_serial;
 display_generation=p67_blur_display_generation;
 p67_blur_release();
 P67_BLUR_MAINTAIN();
 if(epoch!=p67_blur_epoch || generation!=p67_blur_root_generation ||
    display_generation!=p67_blur_display_generation || request!=p67_blur_request_serial ||
    !p67_blur_capture_allowed()) { p67_blur_core_gate=9; goto done; }
 if(!p67_blur_display_valid()) { p67_blur_core_gate=4; goto done; }
 ++p67_blur_copies;
 if(!P67_BLUR_COPY(p67_blur_plane,&identity)) {
  p67_blur_core_gate=(epoch!=p67_blur_epoch || generation!=p67_blur_root_generation ||
   display_generation!=p67_blur_display_generation || request!=p67_blur_request_serial ||
   !p67_blur_capture_allowed())?9u:7u;
  goto done;
 }
 if(epoch!=p67_blur_epoch || generation!=p67_blur_root_generation ||
    display_generation!=p67_blur_display_generation || request!=p67_blur_request_serial ||
    !p67_blur_capture_allowed()) { p67_blur_core_gate=9; goto done; }
 if(!p67_blur_display_valid()) { p67_blur_core_gate=4; goto done; }
 if(!p67_blur_root_registered) { p67_blur_core_gate=5; goto done; }
 if(identity!=p67_blur_root || identity!=P67_BLUR_WATCHFACE()) { p67_blur_core_gate=6; goto done; }
 (void)rh_blur_filter_rgb565(p67_blur_plane,p67_blur_work,P67_BLUR_WIDTH,P67_BLUR_HEIGHT,2,2,P67_BLUR_DARKEN_PERCENT);
 next=P67_BLUR_PUBLISH(p67_blur_plane,epoch);
 if(!next) {
  p67_blur_core_gate=(epoch!=p67_blur_epoch || generation!=p67_blur_root_generation ||
   display_generation!=p67_blur_display_generation || request!=p67_blur_request_serial ||
   !p67_blur_capture_allowed())?9u:8u;
  goto done;
 }
 if(epoch!=p67_blur_epoch || generation!=p67_blur_root_generation ||
    display_generation!=p67_blur_display_generation || request!=p67_blur_request_serial ||
    !p67_blur_capture_allowed()) p67_blur_core_gate=9;
 else if(!p67_blur_display_valid()) p67_blur_core_gate=4;
 else if(!p67_blur_root_registered) p67_blur_core_gate=5;
 else if(identity!=P67_BLUR_WATCHFACE()) p67_blur_core_gate=6;
 if(p67_blur_core_gate) { P67_BLUR_RETIRE(next); goto done; }
 ++p67_blur_publishes;
 old=p67_blur_source; p67_blur_source=next;
 if(old && old!=next) P67_BLUR_RETIRE(old);
 if(request==p67_blur_request_serial) {
  p67_blur_pending=0; p67_blur_armed=0; p67_blur_request_wake=0;
 }
 p67_blur_wake=1; p67_blur_core_gate=10;
done:
 p67_blur_busy=0;
}
/* The UI poll arms, but never executes, render/maintenance work. Cleanup is
 * independent of demand and owns a display observer until its fence succeeds. */
static void p67_blur_poll_rearm(void) {
 p67_blur_cleanup_armed=0;
 if(!P67_BLUR_AVAILABLE) return;
 if(p67_blur_demand) {
  if(p67_blur_capture_allowed() && p67_blur_pending) {
   p67_blur_armed=1; p67_blur_request_wake=0; p67_wake_pending=1;
  }
 } else if(p67_blur_cleanup_pending()) {
  p67_blur_cleanup_armed=1; p67_wake_pending=1;
 }
}
/* Safe owner poll registers observers, but never captures, fences, or frees. */
static void p67_blur_watch(int demand) {
 uint32_t display,epoch;
 if(p67_blur_busy || p67_blur_polling) return;
 if(demand && !p67_blur_demand && !p67_blur_entry_scoped && !p67_blur_pending)
  p67_blur_request();
 p67_blur_demand=!!demand;
 if(!demand) {
  p67_blur_visible=0;
  if(p67_blur_source) p67_blur_drop();
  p67_blur_pending=0; p67_blur_armed=0; p67_blur_request_wake=0;
  if(p67_blur_wake) { p67_wake_pending=1; p67_blur_wake=0; }
 }
 /* Rearm before owner gates too. Only render START can spend either budget. */
 p67_blur_poll_rearm();
 if(!P67_BLUR_OWNER_SAFE()) return;
 p67_blur_polling=1; epoch=p67_blur_config_epoch;
 p67_blur_release(); /* Logical retirement only; never fence during UI poll. */
 if(epoch!=p67_blur_config_epoch) goto done;
 if(!demand) {
  if(p67_blur_root) p67_blur_root_unbind();
  if(epoch!=p67_blur_config_epoch) goto done;
  if(!p67_blur_cleanup_pending()) {
   if(p67_blur_display) p67_blur_display_unbind();
   goto done;
  }
  /* Keep/rebind only the display observer: root identity is irrelevant to
   * cleanup of module-owned input and retired outputs. */
 } else {
  if(!P67_BLUR_AVAILABLE) goto done;
  p67_blur_root_watch();
  if(epoch!=p67_blur_config_epoch) goto done;
 }
 if(!P67_BLUR_AVAILABLE) goto done;
 display=P67_BLUR_DISPLAY();
 if(p67_blur_display && !p67_blur_display_valid()) {
  p67_blur_display_unbind(); p67_blur_drop();
 }
 if(display && P67_BLUR_DISPLAY_LIVE(display) && !p67_blur_display && !p67_blur_display_record(display)) {
  uint32_t generation=++p67_blur_display_serial;
  p67_blur_display=display; p67_blur_display_generation=generation;
  if(!P67_BLUR_DISPLAY_ADD(display,p67_blur_display_event,generation) ||
     !p67_blur_display_valid() || epoch!=p67_blur_config_epoch) {
   if(p67_blur_display==display && p67_blur_display_generation==generation) p67_blur_display_unbind();
  }
 }
done:
 if(!p67_blur_demand) {
  p67_blur_pending=0; p67_blur_armed=0; p67_blur_request_wake=0;
  if(p67_blur_wake) { p67_wake_pending=1; p67_blur_wake=0; }
 }
 p67_blur_poll_rearm();
 p67_blur_polling=0;
}

#if defined(RH_P67_BLUR_DIAGNOSTICS) && RH_P67_BLUR_DIAGNOSTICS
/* Provider contract: pure integer reads only, e.g.
 * #define P67_BLUR_NATIVE_DIAGNOSTICS(X) X("native_error", native_error);
 * No native calls, pipeline mutation, formatting, or I/O in the provider macro.
 * The trailing semicolon is accepted, but not required. */
#ifndef P67_BLUR_NATIVE_DIAGNOSTICS
#define P67_BLUR_NATIVE_DIAGNOSTICS(X)
#endif
struct p67_blur_diag_writer { char *out; uint32_t used,capacity; unsigned failed; };
static void p67_blur_diag_text(struct p67_blur_diag_writer *w,const char *text) {
 while(*text) {
  if(w->used==UINT32_MAX || (w->out && w->used>=w->capacity-1u)) {
   w->failed=1; return;
  }
  if(w->out) w->out[w->used]=*text;
  ++w->used; ++text;
 }
}
/* Target words/pointers are 32-bit. Avoid a 64-bit division runtime import in
 * the freestanding zero-import module, including in this diagnostic path. */
static void p67_blur_diag_value(struct p67_blur_diag_writer *w,const char *key,uint32_t value) {
 char digits[10]; unsigned n=0;
 p67_blur_diag_text(w,key); p67_blur_diag_text(w,"=");
 do { digits[n++]=(char)('0'+value%10u); value/=10u; } while(value);
 while(n) {
  char digit[2]={digits[--n],0}; p67_blur_diag_text(w,digit);
 }
 p67_blur_diag_text(w,"\n");
}
/* NUL-terminated text; byte count excludes NUL. Insufficient capacity returns
 * zero without touching out. This getter neither queries natives nor advances
 * the pipeline. The module's UI poll is responsible for any external writes. */
uint32_t rh_platform_p67_blur_diagnostics(char *out,uint32_t capacity) {
 static const char *const keys[]={"core_gate","demand","pending","armed","source",
  "root","root_generation","display","frame52","attempts","copies","publishes",
  "wakes","draws","config_epoch","cleanup_armed","begin_skips"};
 const uint32_t values[]={p67_blur_core_gate,p67_blur_demand,p67_blur_pending,
  p67_blur_armed,(uint32_t)p67_blur_source,p67_blur_root,p67_blur_root_generation,
  p67_blur_display,p67_blur_frame52,p67_blur_attempts,p67_blur_copies,
  p67_blur_publishes,p67_blur_wakes,p67_blur_draws,p67_blur_config_epoch,p67_blur_cleanup_armed,
  p67_blur_begin_skips};
 struct p67_blur_diag_writer writer={0,0,capacity,0};
 unsigned pass,i;
 if(!out || !capacity) return 0;
 for(pass=0;pass<2;++pass) {
  writer.used=0;
  p67_blur_diag_text(&writer,"P67BLUR1\n");
  for(i=0;i<sizeof(values)/sizeof(values[0]);++i)
   p67_blur_diag_value(&writer,keys[i],values[i]);
#define X(key,value) p67_blur_diag_value(&writer,(key),(uint32_t)(value));
  P67_BLUR_NATIVE_DIAGNOSTICS(X)
#undef X
  if(!pass) {
   if(writer.failed || writer.used>=capacity) return 0;
   writer.out=out;
  }
 }
 if(writer.failed) return 0;
 out[writer.used]=0;
 return writer.used;
}
#endif
