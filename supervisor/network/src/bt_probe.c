#include "bt_probe.h"
#include "bt_addresses.h"
#include <stddef.h>
#include <string.h>
#include "../third_party/tinypan/src/tinypan_bnep.h"

/* No UI calls or blocking IPC in the BT callbacks. The MCU is single-core;
 * short interrupt-masked copies protect snapshots without priority inversion. */
static uint32_t lock(void) {uint32_t p;__asm__ volatile("mrs %0, primask\ncpsid i":"=r"(p)::"memory");return p;}
static void unlock(uint32_t p) {__asm__ volatile("msr primask, %0"::"r"(p):"memory");}
#define FN(n,t) ((t)(uintptr_t)(n))
enum { IDLE, SCAN, PAIR, CONNECTING, LINK, STOPPING };
#define MAX_DEVICES 16u
struct state {
    uint32_t phase,count,selected,adapter_state,bond,pair_confirm,passkey;
    uint32_t event,events,rx,tx,flow,bnep,bnep_code,frames,abort,work_pending;
    uint32_t ticks,started,connect_ready,scan_stopped,pair_reply,ui_taps,ui_action;
    uint32_t scan_stage,scan_seen_start,discovery_callbacks,discovery_value;
    int32_t scan_result;
    uint32_t filter_result;
    uint32_t error_stage,last_command;
    int32_t ipc_result,service_result,sys_errno;
    uint32_t client_state;int32_t client_fd;
    int32_t error;
    uint16_t cid,mtu;
    struct bt_probe_device devices[MAX_DEVICES];
};
static struct state state={.client_fd=-1,.service_result=-1};
static uint32_t callbacks[17],registration;
static void *adapter;
static uint8_t target[6];
static int same(const uint8_t *a,const uint8_t *b)
{for(unsigned i=0;i<6;i++)if(a[i]!=b[i])return 0;return 1;}
static uint16_t rd16(const uint8_t *p) {return p[0]|((uint16_t)p[1]<<8);}
static void wr16(uint8_t *p,uint16_t v) {p[0]=(uint8_t)v;p[1]=(uint8_t)(v>>8);}
static void wr32(uint8_t *p,uint32_t v) {for(unsigned i=0;i<4;i++)p[i]=(uint8_t)(v>>(8*i));}
static void *alloc(uint32_t n) {return FN(BT_ALLOC,void *(*)(uint32_t))(n);}
static void release(void *p) {FN(BT_FREE,void (*)(void *))(p);}
static void error(int value) {uint32_t p=lock();state.error=value;unlock(p);}
static void pair_error(unsigned stage,int value)
{uint32_t p=lock();state.error_stage=stage;state.error=value;state.phase=IDLE;unlock(p);}
static int exchange(uint32_t command,const void *payload,unsigned length,void *reply,unsigned reply_length)
{
    if(!adapter || length>648 || reply_length>648)return -601;
    uint32_t message[177]={0};if(length)memcpy((uint8_t *)message+60,payload,length);
    int *os_error=FN(BT_ERRNO_LOCATION,int *(*)(void))();
    if(os_error)*os_error=0;
    int ipc=FN(BT_SOCKET_CLIENT_SENDRECV,int (*)(void *,void *,uint32_t))(adapter,message,command);
    int saved_errno=os_error?*os_error:0;
    int result=ipc?-1:(int)(message[3]&255);
    uint32_t p=lock();state.last_command=command;state.ipc_result=ipc;state.service_result=result;state.sys_errno=ipc?saved_errno:0;unlock(p);
    if(ipc)return -618;
    if(!result && reply_length)memcpy(reply,(uint8_t *)message+60,reply_length);
    return result;
}
static int request(uint32_t command,const void *payload,unsigned length)
{return exchange(command,payload,length,0,0);}

static void on_adapter(void *cookie,int value)
{(void)cookie;uint32_t p=lock();state.adapter_state=(uint32_t)value;unlock(p);}
static void on_scan(void *cookie,int value)
{
    (void)cookie;uint32_t p=lock();state.discovery_callbacks++;state.discovery_value=(uint32_t)value;
    state.scan_stopped=value==0;
    if(state.phase==SCAN) {
        if(value){state.scan_seen_start=1;state.scan_stage=6;}
        /* Registration can publish an initial STOPPED snapshot after the
         * request. Do not mistake that snapshot for the end of this scan. */
        else if(state.scan_seen_start){state.phase=IDLE;state.scan_stage=7;}
    }
    unlock(p);
}
static void on_device(void *cookie,const uint8_t *data)
{
    (void)cookie;if(!data)return;
    uint32_t p=lock();
    if(state.phase!=SCAN) {unlock(p);return;}
    state.scan_seen_start=1;state.scan_stage=6;
    unsigned i;for(i=0;i<state.count;i++)if(same(data,state.devices[i].address))break;
    if(i==MAX_DEVICES){unlock(p);return;}
    if(i==state.count)state.count++;
    struct bt_probe_device *d=&state.devices[i];memcpy(d->address,data,6);d->rssi=(int8_t)data[6];
    /* Callback's borrowed discovery payload: 12-byte header + bounded name. */
    unsigned j=0;for(;j<20 && data[12+j];j++)d->name[j]=(char)data[12+j];d->name[j]=0;
    unlock(p);
}
static void on_pair(void *cookie,const uint8_t *address)
{
    (void)cookie;if(!address)return;uint32_t p=lock();
    if(state.phase==PAIR && !state.abort && same(address,target))state.pair_reply=1;
    unlock(p);
}
static void on_display(void *cookie,const uint8_t *address,int transport,int kind,uint32_t passkey)
{
    (void)cookie;(void)kind;if(!address || transport!=1)return;
    uint32_t p=lock();if(state.phase==PAIR && !state.abort && same(address,target)) {state.passkey=passkey;state.pair_confirm=1;}unlock(p);
}
static void on_bond(void *cookie,const uint8_t *address,int transport,int value)
{
    (void)cookie;if(!address || transport!=1)return;
    uint32_t p=lock();
    if(state.phase==PAIR && same(address,target)) {
        state.bond=(uint32_t)value;
        if(value==2 && !state.abort)state.connect_ready=1;
        else if(value==0 || state.abort){state.phase=IDLE;state.error=-611;state.error_stage=6;}
    }
    unlock(p);
}
static int prepare(void)
{
    uint32_t p=lock();state.scan_stage=1;unlock(p);
    if(!adapter){
        p=lock();state.client_state=1;unlock(p);
        /* This runs in chaos.net. The firmware constructor creates the
         * AF_LOCAL Bluetooth client, its private receive loop and the
         * current task-group registration. */
        adapter=FN(BT_CLIENT_CREATE,void *(*)(void))();
        p=lock();state.client_state=adapter?2:3;
        state.client_fd=adapter?*(int32_t *)((uint8_t *)adapter+0x30):-1;unlock(p);
        if(!adapter){pair_error(1,-601);return -1;}
    }
    if(!registration){
        callbacks[0]=(uint32_t)(uintptr_t)on_adapter;callbacks[1]=(uint32_t)(uintptr_t)on_scan;
        callbacks[2]=(uint32_t)(uintptr_t)on_device;callbacks[5]=(uint32_t)(uintptr_t)on_pair;
        callbacks[6]=(uint32_t)(uintptr_t)on_display;callbacks[9]=(uint32_t)(uintptr_t)on_bond;
        p=lock();state.scan_stage=2;unlock(p);
        registration=FN(BT_ADAPTER_REGISTER,uint32_t (*)(void *,const uint32_t *))(adapter,callbacks);
        if(!registration){p=lock();state.client_state=4;unlock(p);pair_error(1,-602);return -1;}
    }
    p=lock();state.scan_stage=3;unlock(p);
    int value=request(27,0,0);
    if(value<0){pair_error(1,value);return -1;}
    on_adapter(0,value);
    if(value!=4){pair_error(1,-619);return -1;}
    return 0;
}

int bt_probe_local_address(uint8_t address[6])
{
    if(!address)return -22;
    if(prepare())return -601;
    return exchange(33,0,0,address,6);
}

static int l2_event(uint32_t event,void *argument);
static void queue(uint32_t command);
typedef int (*work_fn)(int,int,void *);
static int cancel_work(int valid,int event,void *argument)
{(void)valid;(void)event;uint32_t command=*(uint32_t *)argument;release(argument);uint32_t p=lock();state.work_pending=0;state.error=-603;if(command==1 && !state.cid)state.phase=IDLE;unlock(p);return 0;}
static int run_work(int valid,int event,void *argument)
{
    (void)valid;(void)event;uint32_t command=*(uint32_t *)argument;release(argument);
    uint32_t p=lock();state.work_pending=0;uint16_t cid=state.cid;uint32_t phase=state.phase,abort=state.abort;unlock(p);
    if(command==1 && phase==CONNECTING && !abort) {
        uint8_t *r=alloc(68);if(!r){error(-604);p=lock();state.phase=IDLE;unlock(p);return 0;}
        memset(r,0,68);wr16(r+2,0x000f);wr32(r+12,(uint32_t)(uintptr_t)l2_event);
        memcpy(r+16,target,6);wr16(r+52,1691);wr16(r+54,1);
        uint32_t queued=FN(BT_L2CAP_CONNECT,uint32_t (*)(const void *))(r);
        if(!queued){release(r);error(-605);p=lock();state.phase=IDLE;unlock(p);}
    } else if(command==1 && abort && !cid) {
        p=lock();state.phase=IDLE;unlock(p);
    } else if(command==2 && cid>0x3f) {
        uint8_t *r=alloc(4);if(!r){error(-604);return 0;}
        wr16(r,cid);wr16(r+2,0);
        /* The disconnect wrapper transfers request ownership to its queue. */
        if(!FN(BT_L2CAP_DISCONNECT,int (*)(const void *))(r)){release(r);error(-605);}
    } else if(command==3 && phase==LINK && !abort) {
        p=lock();state.bnep_code=0xffff;state.started=state.ticks;unlock(p);
        bnep_on_l2cap_connected(); /* TinyPAN sends PANU -> NAP setup. */
    }
    /* A page-close/disconnect may overtake a queued connect/BNEP command. */
    if(command!=2 && abort && cid>0x3f)queue(2);
    return 0;
}
static void queue(uint32_t command)
{
    void *owner=*(void *volatile *)(uintptr_t)BT_OWNER_SLOT;
    if(!owner){uint32_t p=lock();state.error=-606;if(command==1 && !state.cid)state.phase=IDLE;unlock(p);return;}
    uint32_t p=lock();if(state.work_pending){state.error=-607;unlock(p);return;}state.work_pending=1;unlock(p);
    uint32_t *token=alloc(4);if(!token){p=lock();state.work_pending=0;state.error=-604;unlock(p);return;}
    *token=command;
    /* Native queue has no reliable boolean result. It calls cancel itself
     * when unavailable; run/cancel each consume exactly one token. */
    FN(BT_QUEUE_EXTERNAL,int (*)(void *,work_fn,work_fn,void *,uint8_t))(owner,run_work,cancel_work,token,0x71);
}
int hal_bt_l2cap_send(const uint8_t *data,uint16_t length)
{
    uint32_t p=lock();uint16_t cid=state.cid,mtu=state.mtu;uint32_t phase=state.phase,abort=state.abort;unlock(p);
    if(phase!=LINK || abort || cid<=0x3f || !length || length>mtu){error(-608);return -608;}
    /* Mirror audited StockBuffer layout; unlike stock constructor, check OOM. */
    uint32_t total=(uint32_t)length+16;uint8_t *buffer=alloc(total+6);
    if(!buffer){error(-604);return -604;}
    memset(buffer,0,total+6);wr16(buffer,(uint16_t)total);wr16(buffer+2,16);buffer[8]=1;
    memcpy(buffer+20,data,length);
    if(!FN(BT_L2CAP_SUBMIT_CID,int (*)(void *,uint16_t))(buffer,cid)){release(buffer);error(-605);return -605;}
    p=lock();state.tx++;unlock(p);return 0;
}
void hal_bt_l2cap_request_can_send_now(void) { /* Next BT flow/data event drains TinyPAN control queue. */ }
static void bnep_changed(bnep_state_t value,void *user)
{(void)user;uint32_t p=lock();state.bnep=(uint32_t)value;unlock(p);}
static void bnep_result(uint16_t value,void *user)
{(void)user;uint32_t p=lock();state.bnep_code=value;if(value)state.error=-620-(int)value;unlock(p);}
static void filter_result(uint16_t value,void *user)
{(void)user;uint32_t p=lock();state.filter_result=value;unlock(p);}
static void ethernet_frame(const bnep_ethernet_frame_t *frame,void *user)
{(void)user;(void)frame;uint32_t p=lock();state.frames++;unlock(p);/* pan_tinypan_tap forwards to the private netif. */}
static int l2_event(uint32_t event,void *argument)
{
    if(!argument)return 0;uint8_t *a=argument;
    uint32_t p=lock();state.event=event;state.events++;
    uint16_t cid=state.cid;uint32_t abort=state.abort;unlock(p);
    if(event==2) {
        uint16_t new_cid=rd16(a);p=lock();state.cid=new_cid;unlock(p);
        if(abort && new_cid>0x3f)queue(2);
    } else if(event==3) {
        uint16_t new_cid=rd16(a+108),mtu=rd16(a+72);
        if(new_cid!=cid || new_cid<=0x3f){error(-609);}
        else {
            bnep_init();bnep_register_state_callback(bnep_changed,0);
            bnep_register_setup_response_callback(bnep_result,0);bnep_register_frame_callback(ethernet_frame,0);
            bnep_register_filter_response_callback(filter_result,0);
            p=lock();state.mtu=mtu;state.filter_result=0xffff;state.phase=abort ? STOPPING : LINK;unlock(p);
            if(abort || mtu<1691){if(mtu<1691)error(-610);queue(2);}
        }
    } else if(event==6) {
        if(rd16(a)==cid){bnep_on_l2cap_disconnected();p=lock();state.cid=0;state.mtu=0;state.phase=IDLE;state.abort=0;unlock(p);}
    } else if(event==7) {
        unsigned total=rd16(a),offset=rd16(a+2);
        if(rd16(a+4)!=cid || total<offset || offset<6 || offset>64 || total-offset>1691)error(-612);
        else {p=lock();state.rx++;unlock(p);bnep_handle_incoming(a+4+offset,(uint16_t)(total-offset));}
    } else if(event==8) {p=lock();state.flow++;unlock(p);}
    if(event==7 || event==8)bnep_drain_control_tx_queue();
    release(argument);return 0;
}
void bt_probe_scan(void)
{
    if(prepare())return;uint32_t p=lock();
    if(state.phase!=IDLE){state.error=-607;unlock(p);return;}
    state.count=state.selected=0;state.error=0;state.phase=SCAN;state.started=state.ticks;
    state.scan_stopped=state.scan_seen_start=0;state.scan_stage=4;state.scan_result=0;unlock(p);
    uint32_t timeout=20;int rc=request(30,&timeout,sizeof(timeout));
    p=lock();state.scan_result=rc;
    if(rc){state.error=rc;state.error_stage=1;state.phase=IDLE;state.scan_stage=8;}
    else if(state.scan_stage==4)state.scan_stage=5;
    unlock(p);
}
void bt_probe_next(void)
{uint32_t p=lock();if(state.count && (state.phase==IDLE || state.phase==SCAN))state.selected=(state.selected+1)%state.count;unlock(p);}
void bt_probe_action(uint32_t id)
{uint32_t p=lock();state.ui_taps++;state.ui_action=id;unlock(p);}
void bt_probe_view(struct bt_probe_view *out)
{
    uint32_t p=lock();out->phase=state.phase;out->count=state.count;out->bnep=state.bnep;
    out->pair_confirm=state.pair_confirm;out->passkey=state.passkey;out->scan_stage=state.scan_stage;
    out->error=state.error;out->error_stage=state.error_stage;out->cid=state.cid;out->mtu=state.mtu;unlock(p);
}
void bt_probe_diagnostics(struct bt_probe_diagnostics *out)
{
    if(!out)return;
    uint32_t p=lock();
    out->adapter_state=state.adapter_state;out->scan_stage=state.scan_stage;
    out->scan_events=state.discovery_callbacks;out->scan_seen_start=state.scan_seen_start;
    out->command=state.last_command;out->ipc_result=state.ipc_result;
    out->service_result=state.service_result;out->sys_errno=state.sys_errno;
    out->client_state=state.client_state;out->client_fd=state.client_fd;
    out->l2_event=state.event;out->l2_events=state.events;
    out->error=state.error;out->error_stage=state.error_stage;unlock(p);
}
_Static_assert(sizeof(struct bt_probe_diagnostics)==56,"Rust diagnostics ABI");
int bt_probe_device(uint32_t index,struct bt_probe_device *out)
{uint32_t p=lock();int ok=index<state.count;if(ok)memcpy(out,&state.devices[index],sizeof(*out));unlock(p);return ok;}
void bt_probe_connect_device(uint32_t index)
{
    uint32_t p=lock();int ok=index<state.count && (state.phase==IDLE || state.phase==SCAN);
    if(ok)state.selected=index;unlock(p);if(ok)bt_probe_connect();
}
void bt_probe_connect(void)
{
    if(prepare())return;uint32_t p=lock();
    if(state.phase==PAIR && state.pair_confirm) {
        state.pair_confirm=0;unlock(p);
        uint8_t payload[8];memcpy(payload,target,6);payload[6]=payload[7]=1;
        error(request(98,payload,sizeof(payload)));return;
    }
    if((state.phase!=IDLE && state.phase!=SCAN) || !state.count){state.error=-607;unlock(p);return;}
    int scanning=state.phase==SCAN;
    memcpy(target,state.devices[state.selected].address,6);state.phase=PAIR;state.abort=0;state.error=0;state.error_stage=0;
    state.started=state.ticks;state.cid=state.mtu=0;state.bond=state.pair_confirm=state.connect_ready=state.pair_reply=0;
    state.bnep=0;state.bnep_code=0xffff;unlock(p);
    if(scanning){p=lock();state.scan_stopped=0;unlock(p);int rc=request(31,0,0);if(rc){pair_error(1,rc);return;}}
    /* Defer bond IPC until the discovery-stopped callback. */
    if(!scanning){p=lock();state.scan_stopped=1;unlock(p);}
}
void bt_probe_disconnect(void)
{
    uint32_t p=lock();uint32_t phase=state.phase;state.abort=1;state.connect_ready=0;uint16_t cid=state.cid;
    if(phase==LINK || phase==CONNECTING)state.phase=STOPPING;unlock(p);
    if(phase==SCAN){int rc=request(31,0,0);if(rc)error(rc);}
    else if(cid>0x3f)queue(2);
    else if(phase==PAIR){p=lock();state.phase=IDLE;unlock(p);}
}
void bt_probe_bnep(void)
{uint32_t p=lock();int allowed=state.phase==LINK && state.mtu>=1691 && !state.bnep;unlock(p);if(allowed)queue(3);else error(-613);}
void bt_probe_leave(void) {bt_probe_disconnect();}
void bt_probe_poll(void)
{
    uint32_t p=lock();state.ticks++;
    if(state.phase==SCAN && state.ticks-state.started>88) {
        state.phase=IDLE;state.scan_stage=7;
        if(!state.scan_seen_start){state.error=-617;state.scan_stage=9;}
    }
    int bond=state.phase==PAIR && state.scan_stopped && !state.bond && !state.abort;
    if(bond)state.bond=1;
    int connect=state.phase==PAIR && state.connect_ready && !state.abort;
    int pair_reply=state.phase==PAIR && state.pair_reply && !state.abort;state.pair_reply=0;
    if(connect){state.phase=CONNECTING;state.connect_ready=0;state.started=state.ticks;}
    if((state.phase==CONNECTING || state.phase==PAIR) && state.ticks-state.started>120)state.error=-614;
    if(state.phase==LINK && state.bnep==BNEP_STATE_WAIT_FOR_CONNECTION_RESPONSE && state.ticks-state.started>40)state.error=-616;
    unlock(p);
    if(pair_reply){uint8_t payload[7];memcpy(payload,target,6);payload[6]=1;error(request(97,payload,sizeof(payload)));}
    if(bond){
        /* Match the reference's bondability preflight. Aggregate bond state
         * may describe an LE bond: query this Classic transport separately. */
        uint8_t payload[8]={0};memcpy(payload,target,6);payload[6]=1;
        int pairing=request(92,payload,8);
        p=lock();int ipc=state.ipc_result;unlock(p);
        if(ipc || pairing<0 || pairing>2){pair_error(4,pairing);return;}
        if(pairing==2){p=lock();state.bond=2;state.connect_ready=1;unlock(p);}
        else if(pairing==0) {
            int mode=request(38,0,0);
            p=lock();ipc=state.ipc_result;unlock(p);
            if(ipc || mode<0 || mode>3){pair_error(2,mode);return;}
            uint8_t settings[2]={(uint8_t)mode,1};
            int rc=request(37,settings,2);
            if(rc){pair_error(3,rc);return;}
            rc=request(94,payload,8);
            if(rc){pair_error(5,rc);return;}
        } /* pairing==1: already bonding; wait for the matching callback. */
    }
    if(connect)queue(1);
}
/* Bounded formatting: no dependency on an unaudited firmware printf. */
struct output {char *s;uint32_t n,cap;};
static void chr(struct output *o,char c) {if(o->n+1<o->cap)o->s[o->n++]=c;}
static void str(struct output *o,const char *s) {while(*s)chr(o,*s++);}
static void num(struct output *o,uint32_t v,unsigned base,unsigned width)
{char b[16];unsigned n=0;do{b[n++]="0123456789ABCDEF"[v%base];v/=base;}while(v && n<16);while(n<width)b[n++]='0';while(n)chr(o,b[--n]);}
void bt_probe_text(char *out,uint32_t capacity)
{
    struct state s;uint32_t p=lock();memcpy(&s,&state,sizeof(s));unlock(p);
    struct output o={out,0,capacity};static const char *const phases[]={"Idle","Scanning","Pair phone","Connecting","L2CAP ready","Stopping"};
    str(&o,phases[s.phase]);str(&o,"\nBT:");num(&o,s.adapter_state,10,1);str(&o," Tap:");num(&o,s.ui_taps,10,1);chr(&o,'/');num(&o,s.ui_action,10,1);str(&o,"\n");
    str(&o,"Scan:");num(&o,s.scan_stage,10,1);str(&o," rc:");if(s.scan_result<0)chr(&o,'-');num(&o,s.scan_result<0?0u-(uint32_t)s.scan_result:(uint32_t)s.scan_result,10,1);
    str(&o," d:");num(&o,s.discovery_value,10,1);chr(&o,'/');num(&o,s.discovery_callbacks,10,1);chr(&o,'\n');
    if(s.count){struct bt_probe_device *d=&s.devices[s.selected];num(&o,s.selected+1,10,1);str(&o,"/");num(&o,s.count,10,1);str(&o," ");str(&o,d->name);str(&o,"\n");for(unsigned i=0;i<6;i++){if(i)chr(&o,':');num(&o,d->address[5-i],16,2);}str(&o,"\n");}
    else str(&o,"Scan and select phone\n");
    str(&o,"CID:");num(&o,s.cid,16,4);str(&o," MTU:");num(&o,s.mtu,10,1);
    str(&o,"\nEvent:");num(&o,s.event,10,1);str(&o," #");num(&o,s.events,10,1);
    str(&o,"\nRX:");num(&o,s.rx,10,1);str(&o," TX:");num(&o,s.tx,10,1);str(&o," F:");num(&o,s.flow,10,1);
    str(&o,"\nBNEP:");num(&o,s.bnep,10,1);str(&o," rc:");num(&o,s.bnep_code,16,4);
    str(&o," Filter:");num(&o,s.filter_result,16,4);
    str(&o,"\nErr:");if(s.error<0)chr(&o,'-');num(&o,s.error<0 ? 0u-(uint32_t)s.error:(uint32_t)s.error,10,1);
    str(&o," Stage:");num(&o,s.error_stage,10,1);str(&o," Cmd:");num(&o,s.last_command,10,1);
    str(&o," IPC:");num(&o,(uint32_t)s.ipc_result,10,1);str(&o," Svc:");num(&o,(uint32_t)s.service_result,10,1);
    if(s.pair_confirm){str(&o,"\nKey:");num(&o,s.passkey,10,6);str(&o," Confirm?");}
    str(&o,"\nIP: module-private PAN");if(capacity)out[o.n]=0;
}
