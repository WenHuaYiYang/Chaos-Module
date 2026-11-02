/* Host lifecycle model: captures occur only at host_display render-start. */
#include <assert.h>
#include <stdint.h>
#include <string.h>
#include <stdio.h>
#define RH_P67_BLUR_DIAGNOSTICS 1
#include "resource_hook_p67_blur.h"
static uint32_t wf_root=100,record[3],display_record[3],host_display=200;
static unsigned display_count,wakes,wake_fail,wake_reenter,wake_reset,wake_delete;
static unsigned copy_display_delete,publish_display_delete,copy_display_change,publish_display_change;
static unsigned count,adds,copies,publishes,retires,maintains,fail_copy,fail_publish,wf_visible=1;
static unsigned delete_copy,reset_publish,phase,exit_copy,exit_publish;
static unsigned owner_safe=1,owner_checks,native_calls;
static int p67_wake_pending;
static uint16_t output[2][P67_BLUR_PIXELS];
static unsigned occupied[2],retired_output[2];
static unsigned owned_input,pin_copy,cleanup_fail,wrong_identity;
static int rh_background_is_blur(const char *s) { return !strcmp(s,":blur"); }
static int p67_live(uint32_t r) { ++native_calls;return r==wf_root && r; }
static uint32_t *p67_descriptor(uint32_t r,uint32_t cb) { ++native_calls;return r==wf_root && count && record[0]==cb?record:0; }
static uintptr_t native(uint32_t);
static uint32_t get_count(uint32_t);
static uint32_t *get(uint32_t,uint32_t);
static int remove_cb(uint32_t,uint32_t);
static int copy(uint16_t *,uint32_t *);
static uintptr_t publish(const uint16_t *,uint32_t);
static void retire(uintptr_t);
static void maintain(void);
static void *display_add(uint32_t d,void (*cb)(void *),uint32_t generation) {
 assert(d==host_display && !display_count); ++adds; display_count=1;
 display_record[0]=(uint32_t)(uintptr_t)cb; display_record[1]=generation; display_record[2]=0x8000u;return display_record;
}
static int wake(void);
static void event(unsigned);
#define P67_NATIVE(a) native(a)
#define P67_BLUR_AVAILABLE 1
#define P67_BLUR_OWNER_SAFE() (++owner_checks,owner_safe)
#define P67_BLUR_WATCHFACE() wf_root
#define P67_BLUR_DISPLAY() host_display
#define P67_BLUR_DISPLAY_ADD(d,c,g) display_add(d,c,g)
#define P67_BLUR_WAKE() wake()
#define P67_BLUR_DISPLAY_COUNT(d) get_count(d)
#define P67_BLUR_DISPLAY_GET(d,i) get(d,i)
#define P67_BLUR_DISPLAY_REMOVE(d,c) remove_cb(d,(uint32_t)(uintptr_t)c)
#define P67_BLUR_VISIBLE() wf_visible
#define P67_BLUR_COPY(p,i) copy(p,i)
#define P67_BLUR_PUBLISH(p,e) publish(p,e)
#define P67_BLUR_RETIRE(s) retire(s)
#define P67_BLUR_MAINTAIN() maintain()
#define P67_BLUR_CLEANUP_PENDING() (owned_input || retired_output[0] || retired_output[1])
#ifdef HARNESS_NATIVE_DIAGNOSTICS
#define P67_BLUR_NATIVE_DIAGNOSTICS(X) \
 X("native_error", 52) X("native_unsigned", UINT32_MAX) X("native_truncated64", UINT64_MAX)
#endif
#include "../src/p67_blur.inc"
static struct p67_blur_session entry_test_session;
static void deleted(void) {
 uint32_t e[5]={wf_root,wf_root,36,p67_blur_root_generation,0};
 p67_blur_root_event(e); wf_root=0; count=0;
}
static int wake(void) {
 assert(phase==53); ++wakes;
 if(wake_reenter) { wake_reenter=0;event(53); }
 if(wake_reset) { wake_reset=0;p67_blur_reset(); }
 if(wake_delete) { wake_delete=0;event(36);display_count=0; }
 return wake_fail?-1:0; /* Production redraw ABI. */
}
static uint32_t *add(uint32_t r,uint32_t cb,uint32_t filter,uint32_t data) {
 assert(r==wf_root && !count && filter==36); count=1;
 record[0]=cb;record[1]=data;record[2]=filter;return record;
}
static int remove_cb(uint32_t r,uint32_t cb) { if(r==host_display) { assert(cb==display_record[0]);display_count=0;return 1; } assert(r==wf_root && cb==record[0]); count=0;return 1; }
static uint32_t get_count(uint32_t r) { ++native_calls;if(r==host_display) return display_count;assert(r==wf_root);return count; }
static uint32_t *get(uint32_t r,uint32_t i) { ++native_calls;if(r==host_display) { assert(i<display_count);return display_record; } assert(r==wf_root && i<count);return record; }
static uintptr_t native(uint32_t a) {
 ++native_calls;
 switch(a) {
 case 0x0c5883f1u:return (uintptr_t)add;
 case 0x002bf811u:return (uintptr_t)remove_cb;
 case 0x002bf7d5u:return (uintptr_t)get_count;
 case 0x002bf7e9u:return (uintptr_t)get;
 default:assert(0);return 0;
 }
}
static int copy(uint16_t *p,uint32_t *i) {
 assert(phase==52);++copies;*i=wf_root;
 if(delete_copy) { delete_copy=0;deleted();return 1; }
 if(copy_display_delete) { copy_display_delete=0;event(36);display_count=0; }
 if(copy_display_change) { copy_display_change=0;++host_display;display_count=0; }
 if(exit_copy) {
  exit_copy=0;p67_blur_session_event(&entry_test_session,1,77,301,9,0,480,1);
 }
 if(pin_copy) owned_input=1;
 if(fail_copy) return 0;
 if(wrong_identity) { wrong_identity=0;++*i; }
 for(unsigned j=0;j<P67_BLUR_PIXELS;++j) p[j]=0xffff;
 return 1;
}
static uintptr_t publish(const uint16_t *p,uint32_t epoch) {
 (void)p;(void)epoch;assert(phase==52);++publishes;
 if(reset_publish) { reset_publish=0;p67_blur_reset(); }
 if(exit_publish) {
  exit_publish=0;p67_blur_session_event(&entry_test_session,1,77,301,9,0,480,1);
 }
 if(publish_display_delete) { publish_display_delete=0;event(36);display_count=0; }
 if(publish_display_change) { publish_display_change=0;++host_display;display_count=0; }
 if(fail_publish) return 0;
 for(unsigned j=0;j<2;++j) if(!occupied[j]) { occupied[j]=1;return (uintptr_t)output[j]; }
 return 0;
}
static void retire(uintptr_t s) {
 ++retires;
 for(unsigned j=0;j<2;++j) if(s==(uintptr_t)output[j]) { assert(occupied[j]);retired_output[j]=1;return; }
 assert(0);
}
static void maintain(void) {
 assert(phase==52);++maintains;
 if(!owner_safe || cleanup_fail) return;
 owned_input=0;
 for(unsigned j=0;j<2;++j) if(retired_output[j]) {
  assert(occupied[j]);occupied[j]=0;retired_output[j]=0;
 }
}
static void event(unsigned code) { uint32_t e[5]={host_display,host_display,code,p67_blur_display_generation,0};unsigned saved=phase;phase=code;p67_blur_display_event(e);phase=saved; }
/* Model the launcher's consumption of the deferred UI-poll redraw. We do not
 * claim this host model proves GPU/cache safety on a device. */
static void poll(int demand) {
 unsigned c=copies,p=publishes,m=maintains,w=wakes;
 p67_blur_watch(demand);
 assert(copies==c && publishes==p && maintains==m && wakes==w);
 p67_wake_pending=0;
}
static uint64_t diagnostic_value(const char *text,const char *key) {
 char prefix[64]; uint64_t result=0;
 assert(strlen(key)+3<sizeof(prefix));
 snprintf(prefix,sizeof(prefix),"\n%s=",key);
 const char *p=strstr(text,prefix); assert(p); p+=strlen(prefix);
 assert(*p>='0' && *p<='9');
 while(*p>='0' && *p<='9') result=result*10u+(unsigned)(*p++-'0');
 assert(*p=='\n');return result;
}
static void diagnostics(void) {
 char text[1024],again[1024],small[1024];
 unsigned c=copies,p=publishes,r=retires,m=maintains,w=wakes;
 unsigned n=native_calls,o=owner_checks;
 uint32_t serial=p67_blur_request_serial,epoch=p67_blur_epoch;
 unsigned pending=p67_blur_pending,armed=p67_blur_armed,wake_pending=p67_wake_pending;
 unsigned cleanup_armed=p67_blur_cleanup_armed,gate=p67_blur_core_gate;
 unsigned input=owned_input,retired0=retired_output[0],retired1=retired_output[1];
 uintptr_t source=p67_blur_source,retired=p67_blur_retired;
 uint32_t length=rh_platform_p67_blur_diagnostics(text,sizeof(text));
 assert(length && length==strlen(text) && !strncmp(text,"P67BLUR1\n",9));
 assert(diagnostic_value(text,"core_gate")==gate);
 assert(diagnostic_value(text,"cleanup_armed")==cleanup_armed);
 assert(diagnostic_value(text,"demand")==p67_blur_demand);
 assert(diagnostic_value(text,"pending")==pending);
 assert(diagnostic_value(text,"armed")==armed);
 assert(diagnostic_value(text,"source")==((uint32_t)source)); /* Firmware pointer width, even on host. */
 assert(diagnostic_value(text,"root")==p67_blur_root);
 assert(diagnostic_value(text,"root_generation")==p67_blur_root_generation);
 assert(diagnostic_value(text,"display")==p67_blur_display);
 assert(diagnostic_value(text,"frame52")==p67_blur_frame52);
 assert(diagnostic_value(text,"attempts")==p67_blur_attempts);
 assert(diagnostic_value(text,"copies")==c);
 assert(diagnostic_value(text,"publishes")==p67_blur_publishes);
 assert(diagnostic_value(text,"wakes")==w);
 assert(diagnostic_value(text,"draws")==p67_blur_draws);
 assert(diagnostic_value(text,"config_epoch")==p67_blur_config_epoch);
 assert(diagnostic_value(text,"begin_skips")==p67_blur_begin_skips);
#ifdef HARNESS_NATIVE_DIAGNOSTICS
 assert(diagnostic_value(text,"native_error")==52);
 assert(diagnostic_value(text,"native_unsigned")==UINT32_MAX);
 assert(diagnostic_value(text,"native_truncated64")==UINT32_MAX); /* Only firmware words are exported. */
#else
 assert(!strstr(text,"native_error="));
#endif
 for(uint32_t cap=0;cap<=length;++cap) {
  memset(small,0x5a,sizeof(small));
  assert(!rh_platform_p67_blur_diagnostics(small,cap));
  for(unsigned j=0;j<sizeof(small);++j) assert(small[j]==0x5a);
 }
 assert(!rh_platform_p67_blur_diagnostics(0,sizeof(text)));
 memset(again,0x5a,sizeof(again));
 assert(rh_platform_p67_blur_diagnostics(again,length+1)==length);
 assert(!strcmp(text,again) && again[length+1]==0x5a);
 assert(copies==c && publishes==p && retires==r && maintains==m && wakes==w);
 assert(native_calls==n && owner_checks==o && p67_blur_request_serial==serial);
 assert(p67_blur_epoch==epoch && p67_blur_pending==pending && p67_blur_armed==armed);
 assert(p67_wake_pending==(int)wake_pending && p67_blur_source==source && p67_blur_retired==retired);
 assert(p67_blur_core_gate==gate && p67_blur_cleanup_armed==cleanup_armed);
 assert(owned_input==input && retired_output[0]==retired0 && retired_output[1]==retired1);
}
static void failure_budget(void) {
 uintptr_t old=p67_blur_source;
 unsigned c=copies,w=wakes,a=p67_blur_attempts;
 p67_blur_request();fail_copy=1;
 event(53);event(52); /* A fresh valid request can wake once before START. */
 unsigned n=native_calls,o=owner_checks;
 for(unsigned j=0;j<1000;++j) { event(52);event(53); }
 assert(native_calls==n && owner_checks==o);
 assert(copies==c+1 && wakes==w+1 && p67_blur_attempts==a+1);
 assert(p67_blur_pending && !p67_blur_armed && p67_blur_source==old && p67_blur_core_gate==7);
 diagnostics();
 /* Persistent failure gets one further attempt per poll, no GPU at poll. */
 unsigned m=maintains;
 p67_blur_watch(1);assert(p67_wake_pending && p67_blur_armed);
 assert(copies==c+1 && maintains==m && wakes==w+1);
 p67_wake_pending=0;event(53);event(52);
 for(unsigned j=0;j<1000;++j) { event(52);event(53); }
 assert(copies==c+2 && wakes==w+1 && p67_blur_attempts==a+2);
 /* Recovery needs a poll, not another gesture. */
 fail_copy=0;poll(1);event(52);assert(p67_blur_source && !p67_blur_pending && !p67_blur_armed);
 event(53);c=copies;w=wakes;n=native_calls;o=owner_checks;
 for(unsigned j=0;j<1000;++j) { event(52);event(53);p67_blur_draw_seen(); }
 assert(native_calls==n && owner_checks==o);
 assert(copies==c && wakes==w && p67_blur_draws>=1000);
 /* Consume before owner-safe, not merely before COPY. */
 p67_blur_request();owner_safe=0;a=p67_blur_attempts;
 event(52);for(unsigned j=0;j<1000;++j) { event(52);event(53); }
 assert(copies==c && wakes==w && p67_blur_attempts==a+1 && !p67_blur_armed && p67_blur_core_gate==3);
 p67_blur_watch(1);assert(p67_blur_armed && p67_wake_pending);p67_wake_pending=0;
 event(52);assert(p67_blur_attempts==a+2 && copies==c);
 owner_safe=1;event(52);assert(copies==c);poll(1);event(52);assert(copies==c+1);
 event(53);
 /* Missing root gates are also budgeted. */
 deleted();event(53);wf_root=0;poll(1);c=copies;a=p67_blur_attempts;w=wakes;
 for(unsigned j=0;j<1000;++j) { event(52);event(53); }
 assert(copies==c && wakes==w && p67_blur_attempts==a+1 && p67_blur_core_gate==5);
 wf_root=100;poll(1);event(52);event(53);assert(p67_blur_source);
}
static void cleanup_budget(void) {
 assert(p67_blur_source);
 /* A failed capture may pin an owned input. Demand-off additionally retires
  * the valid output, so both kinds of storage require a successful fence. */
 p67_blur_request();fail_copy=pin_copy=1;event(52);fail_copy=pin_copy=0;
 assert(owned_input && p67_blur_source && p67_blur_core_gate==7);
 cleanup_fail=1;
 unsigned c=copies,p=publishes,m=maintains,w=wakes,a=p67_blur_attempts;
 p67_blur_watch(0);
 assert(!p67_blur_source && !p67_blur_pending && !p67_blur_armed && !count);
 assert(display_count && p67_blur_cleanup_armed && p67_wake_pending);
 assert(copies==c && publishes==p && maintains==m && wakes==w && owned_input);
 assert(retired_output[0] || retired_output[1]);
 p67_wake_pending=0;event(53);event(52);
 assert(maintains==m+1 && p67_blur_attempts==a+1 && p67_blur_core_gate==2);
 assert(P67_BLUR_CLEANUP_PENDING() && !p67_blur_cleanup_armed && display_count);
 unsigned n=native_calls,o=owner_checks;
 for(unsigned j=0;j<1000;++j) { event(52);event(53); }
 assert(copies==c && publishes==p && maintains==m+1 && wakes==w);
 assert(native_calls==n && owner_checks==o && !p67_wake_pending && p67_blur_core_gate==2);
 diagnostics();
 /* A blocked owner consumes the maintenance budget before its safety gate. */
 owner_safe=0;poll(0);assert(p67_blur_cleanup_armed && display_count);
 event(52);assert(maintains==m+1 && p67_blur_core_gate==3 && !p67_blur_cleanup_armed);
 n=native_calls;o=owner_checks;
 for(unsigned j=0;j<1000;++j) { event(52);event(53); }
 assert(native_calls==n && owner_checks==o && maintains==m+1 && wakes==w);
 owner_safe=1;poll(0);event(52);
 assert(maintains==m+2 && P67_BLUR_CLEANUP_PENDING() && p67_blur_core_gate==2);
 /* DELETE/replacement of the observed display must not orphan owned cleanup.
  * Polling rebinds the display without restoring a root observer or demand. */
 event(36);display_count=0;assert(!p67_blur_display);
 poll(0);assert(display_count && !count && !p67_blur_demand && p67_blur_cleanup_armed);
 event(52);assert(maintains==m+3 && P67_BLUR_CLEANUP_PENDING());
 cleanup_fail=0;
 for(unsigned j=0;j<1000;++j) { event(52);event(53); }
 assert(maintains==m+3 && owned_input && (occupied[0] || occupied[1]));
 /* Fence recovery alone is not permission to work every frame. A later poll
  * rearms one cleanup-only START, without requiring another gesture/demand. */
 p67_blur_watch(0);assert(p67_wake_pending && p67_blur_cleanup_armed && maintains==m+3);
 p67_wake_pending=0;event(53);event(52);
 assert(maintains==m+4 && !P67_BLUR_CLEANUP_PENDING() && !p67_blur_cleanup_armed);
 assert(!occupied[0] && !occupied[1] && !owned_input && display_count && p67_blur_core_gate==10);
 assert(copies==c && publishes==p && wakes==w);
 n=native_calls;o=owner_checks;
 for(unsigned j=0;j<1000;++j) { event(52);event(53); }
 assert(native_calls==n && owner_checks==o && maintains==m+4 && wakes==w && p67_blur_core_gate==10);
 diagnostics();
 poll(0);assert(!display_count && !count && !p67_blur_display && !p67_blur_cleanup_armed);
 assert(copies==c && publishes==p && maintains==m+4 && wakes==w);
 /* Return to active demand for subsequent lifecycle tests. */
 poll(1);event(52);event(53);assert(p67_blur_source && p67_blur_core_gate==10);
}
static void entry_policy(void) {
 struct p67_blur_session other={0,0};
 struct p67_blur_session *s=&entry_test_session;
 assert(p67_blur_source);event(53);
 uint32_t serial=p67_blur_request_serial;
 p67_blur_session_event(s,1,77,301,9,480,480,1);
 assert(p67_blur_request_serial==serial+1 && s->generation==77 && s->page==301);
 assert(p67_blur_entry_scoped && p67_blur_allowed_entries==1 && p67_blur_pending);
 serial=p67_blur_request_serial;
 p67_blur_session_event(s,1,77,301,9,480,480,1);
 p67_blur_session_event(s,1,77,301,9,200,480,1);
 assert(p67_blur_request_serial==serial && p67_blur_armed);
 /* First visible START must consume the explicit entry, not request again. */
 p67_blur_visible=0;unsigned c=copies;
 event(52);assert(copies==c+1 && p67_blur_request_serial==serial && !p67_blur_pending);
 event(53);uintptr_t old=p67_blur_source;
 wf_visible=0;event(52);wf_visible=1;event(52);
 assert(copies==c+1 && p67_blur_request_serial==serial && p67_blur_source==old);
 p67_blur_session_event(s,1,77,301,11,200,480,1);
 p67_blur_session_event(s,1,77,301,11,0,480,1);
 assert(s->page==301 && p67_blur_allowed_entries==1);
 /* Failed entry capture still owns only its first begin; partial snap cannot
  * request or rearm after failure. A UI poll may retry until the exit begins. */
 p67_blur_session_event(s,1,77,301,12,480,480,1);
 assert(!p67_blur_allowed_entries && !s->page);
 p67_blur_session_event(s,1,77,301,9,480,480,1);
 fail_copy=1;event(52);c=copies;serial=p67_blur_request_serial;
 for(unsigned n=0;n<10;++n) p67_blur_session_event(s,1,77,301,9,200,480,1);
 event(52);event(53);
 assert(copies==c && p67_blur_request_serial==serial && p67_blur_pending && !p67_blur_armed);
 poll(1);assert(p67_blur_armed);event(52);assert(copies==c+1);
 serial=p67_blur_request_serial;
 p67_blur_session_event(s,1,77,301,9,0,480,0); /* Bubbled open-page content scroll. */
 assert(p67_blur_request_serial==serial && p67_blur_allowed_entries==1);
 assert(p67_blur_pending && !p67_blur_armed && p67_blur_source==old);
 uint32_t epoch=p67_blur_epoch;serial=p67_blur_request_serial;
 p67_blur_session_event(s,1,77,301,9,0,480,1);
 assert(p67_blur_epoch==epoch+1 && p67_blur_request_serial==serial+1);
 assert(!s->page && !p67_blur_allowed_entries && !p67_blur_pending && !p67_blur_armed);
 assert(p67_blur_source==old && !p67_blur_request_wake);
 fail_copy=0;c=copies;unsigned p=publishes,w=wakes;
 serial=p67_blur_request_serial;
 for(unsigned n=0;n<10;++n) {
  poll(1);assert(!p67_wake_pending && !p67_blur_armed);
  wf_visible=n&1;event(52);event(53);
  p67_blur_session_event(s,1,77,301,9,n&1?200:0,480,1);
 }
 wf_visible=1;
 assert(copies==c && publishes==p && wakes==w && p67_blur_request_serial==serial);
 assert(p67_blur_source==old);
 /* Reentrant exit during COPY aborts filtering/publication; during PUBLISH,
  * retire only the rejected result, never the previous immutable source. */
 p67_blur_session_event(s,1,77,301,9,480,480,1);exit_copy=1;event(52);
 assert(copies==c+1 && publishes==p && p67_blur_source==old && !p67_blur_pending);
 p67_blur_session_event(s,1,77,301,9,480,480,1);exit_publish=1;
 unsigned r=retires;event(52);
 assert(publishes==p+1 && retires==r+1 && p67_blur_source==old);
 assert(!p67_blur_allowed_entries && !p67_blur_pending && p67_blur_core_gate==9);
 c=copies;p=publishes;serial=p67_blur_request_serial;
 poll(1);event(52);event(53);
 assert(copies==c && publishes==p && p67_blur_request_serial==serial);
 /* Each viewbit is independent. Exiting one page cannot cancel another's
  * pending work, even if the bindings share an owner (the core has no owner). */
 p67_blur_session_event(s,1,77,301,9,480,480,1);
 p67_blur_session_event(&other,4,88,302,9,-480,-480,1);
 serial=p67_blur_request_serial;epoch=p67_blur_epoch;
 p67_blur_session_event(s,1,77,301,9,0,480,1);
 assert(p67_blur_allowed_entries==4 && p67_blur_pending && p67_blur_armed);
 assert(p67_blur_request_serial==serial && p67_blur_epoch==epoch);
 event(52);assert(p67_blur_source && !p67_blur_pending);event(53);
 p67_blur_session_clear(&other,4);assert(!p67_blur_allowed_entries);
 /* Root/display lifecycle invalidation may leave dirty work while closed,
  * but neither poll nor READY/visible rise may wake/rearm/capture it. */
 deleted();wf_root=100;c=copies;p=publishes;w=wakes;serial=p67_blur_request_serial;
 assert(p67_blur_pending && !p67_blur_armed && !p67_blur_source);
 for(unsigned n=0;n<10;++n) {
  poll(1);assert(!p67_wake_pending && !p67_blur_armed);
  event(52);event(53);
 }
 assert(copies==c && publishes==p && wakes==w && p67_blur_request_serial==serial);
 event(36);display_count=0;
 poll(1);assert(display_count && p67_blur_pending && !p67_blur_armed && !p67_wake_pending);
 event(52);event(53);assert(copies==c && publishes==p && wakes==w);
 p67_blur_session_event(s,1,78,301,9,200,480,1);
 assert(!s->page && !p67_blur_allowed_entries && !p67_blur_armed);
 p67_blur_session_event(s,1,78,301,9,480,480,1);event(52);event(53);
 assert(p67_blur_source && !p67_blur_pending && copies==c+1);
 p67_blur_session_clear(s,1);old=p67_blur_source;
 /* Explicit config reset is the prewarm exception, not an exit request. */
 serial=p67_blur_request_serial;p67_blur_reset();
 assert(!p67_blur_entry_scoped && !p67_blur_allowed_entries && p67_blur_capture_allowed());
 assert(p67_blur_request_serial==serial+1 && p67_blur_source==old);
 p67_blur_visible=0;serial=p67_blur_request_serial;event(52);event(53);
 assert(p67_blur_source && !p67_blur_pending && p67_blur_request_serial==serial);
 /* Enter direction management again before testing demand-off cleanup. */
 p67_blur_session_event(s,1,78,301,9,480,480,1);
 p67_blur_session_event(s,1,78,301,9,0,480,1);
 assert(!p67_blur_capture_allowed() && !p67_blur_pending);
 diagnostics();
}
int main(void) {
 poll(1);poll(1);assert(adds==1 && !copies);
 event(25);assert(!copies);event(52);assert(copies==1 && p67_blur_source && !p67_wake_pending);
 event(53);assert(wakes==1 && !p67_wake_pending);event(52);assert(copies==1);
 failure_budget();
 uintptr_t old=p67_blur_source;
 p67_blur_request();fail_publish=1;event(52);
 assert(p67_blur_source==old && p67_blur_pending && p67_blur_core_gate==8);
 unsigned before=copies;event(52);assert(copies==before);fail_publish=0;poll(1);event(52);
 assert(p67_blur_source && !p67_blur_pending);event(53);
 wf_visible=0;event(52);wf_visible=1;event(52);assert(copies==before+2);event(53);
 p67_blur_request();p67_blur_request();before=copies;event(52);assert(copies==before+1);event(53);
 /* Explicit reload is logical only, including when the owner is unsafe. */
 old=p67_blur_source;before=native_calls;
 uint32_t epoch=p67_blur_epoch,config=p67_blur_config_epoch;
 owner_safe=0;p67_blur_reset();assert(native_calls==before && p67_blur_source==old);
 assert(p67_blur_epoch==epoch+1 && p67_blur_config_epoch==config+1 && p67_blur_armed);
 event(52);owner_safe=1;fail_copy=1;poll(1);event(52);assert(p67_blur_source==old);
 fail_copy=0;poll(1);event(52);event(53);assert(p67_blur_source);
 /* Changed root is checked next safe START, not from reset. */
 old=p67_blur_source;wf_root=101;count=0;p67_blur_reset();assert(p67_blur_source==old);
 before=copies;event(52);assert(p67_blur_root==101 && p67_blur_source && copies==before+1);event(53);
 before=retires;deleted();assert(!p67_blur_source && retires==before);
 /* Same numerical root address is a fresh lifecycle, stale DELETE ignored. */
 uint32_t stale=p67_blur_root_generation-1;wf_root=101;poll(1);
 uint32_t e[5]={101,101,36,stale,0};p67_blur_root_event(e);assert(p67_blur_root_registered);
 event(52);assert(p67_blur_source);event(53);
 p67_blur_request();delete_copy=1;event(52);assert(!p67_blur_source);
 wf_root=101;poll(1);event(52);assert(p67_blur_source);event(53);
 old=p67_blur_source;p67_blur_request();reset_publish=1;event(52);
 assert(p67_blur_source==old && p67_blur_pending && p67_blur_armed && p67_blur_core_gate==9);
 event(52);assert(p67_blur_source && !p67_blur_pending);event(53);
 before=copies;for(unsigned j=0;j<20;++j) { assert(p67_source(":blur"));event(25);event(12); }
 assert(copies==before);poll(0);assert(display_count && !count && p67_blur_cleanup_armed);
 event(52);assert(copies==before && !p67_blur_source && !P67_BLUR_CLEANUP_PENDING() && display_count);
 poll(0);assert(!display_count);
 /* Successful wake consumes only its own token; recursive READY is inert. */
 poll(1);event(52);before=wakes;wake_reenter=1;event(53);
 assert(wakes==before+1 && !p67_blur_wake);
 event(52);assert(!p67_blur_pending);
 old=p67_blur_source;p67_blur_request();event(52);old=p67_blur_source;
 wake_reset=1;event(53);
 assert(p67_blur_source==old && p67_blur_pending && p67_blur_armed && p67_wake_pending);
 p67_wake_pending=0;event(52);assert(p67_blur_source);
 wake_delete=1;event(53);assert(!p67_blur_display && !p67_blur_source && p67_blur_pending && p67_blur_wake);
 poll(1);event(52);assert(p67_blur_source);event(53);
 uint32_t old_generation=p67_blur_display_generation;
 p67_blur_request();copy_display_delete=1;before=publishes;event(52);
 assert(!p67_blur_source && publishes==before && !p67_blur_display);
 poll(1);event(52);assert(p67_blur_source);event(53);
 uint32_t stale_event[5]={host_display,host_display,36,old_generation,0};
 p67_blur_display_event(stale_event);assert(p67_blur_display==host_display);
 p67_blur_request();publish_display_delete=1;event(52);assert(!p67_blur_source && !p67_blur_display);
 poll(1);event(52);assert(p67_blur_source);event(53);
 p67_blur_request();copy_display_change=1;before=publishes;event(52);
 assert(publishes==before && p67_blur_pending && p67_blur_core_gate==4);
 poll(1);event(52);assert(p67_blur_source);event(53);
 p67_blur_request();publish_display_change=1;old=p67_blur_source;event(52);
 assert(p67_blur_source==old && p67_blur_pending);
 poll(1);event(52);assert(p67_blur_source);event(53);
 /* Failed native wake also consumes its budget, with one deferred fallback. */
 p67_blur_request();wake_fail=1;before=wakes;event(53);
 assert(p67_wake_pending);p67_wake_pending=0;
 for(unsigned j=0;j<1000;++j) event(53);
 assert(wakes==before+1 && !p67_wake_pending);wake_fail=0;event(52);event(53);
 diagnostics();
 old=p67_blur_source;p67_blur_request();wrong_identity=1;event(52);
 assert(p67_blur_source==old && p67_blur_core_gate==6 && p67_blur_pending);
 diagnostics();poll(1);event(52);event(53);
 cleanup_budget();
 entry_policy();
 /* Demand-off clears logical state even while owner gates are closed. */
 owner_safe=0;before=native_calls;p67_blur_watch(0);
 assert(!p67_blur_source && !p67_blur_pending && !p67_blur_armed && native_calls==before);
 owner_safe=1;poll(0);assert(display_count && !count && p67_blur_cleanup_armed);
 event(52);assert(!occupied[0] && !occupied[1] && display_count && !p67_blur_cleanup_armed);
 poll(0);assert(!display_count);
 diagnostics();
 puts("p67 active blur lifecycle/retry/diagnostics ok (host model only)");return 0;
}
