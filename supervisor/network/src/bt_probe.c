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
    /* 配对重试(桌面栈会先广播 bond=0 快照, 见 on_bond 注释): 4s/次, 至多6次。
     * bond_addr/bond_order: 043 上建绑定命令要哪种地址序没有设备证据 —— 扫描
     * 回调序与反转序交替重试, 回调比较跟 bond_addr 走, 谁先配成功用谁。 */
    uint32_t bond_attempts,bond_retry_at,bond_order,bond_pending,bonding,bond_events;
    uint32_t bond_last_value,bond_last_transport,pair_reply_result,device_configured;
    int32_t result_cod,result_iocap,result_paircfg,result_connect,result_bond,result_confirm,result_remove,result_name,result_name_read;
    char local_name[32];
    uint32_t bond_remove_pending;
    uint8_t bond_addr[6];
    uint32_t event_head;
    struct probe_event {uint32_t kind,first,second;uint8_t address[6];} history[16];
    struct bt_probe_device devices[MAX_DEVICES];
};
static struct state state={.client_fd=-1,.service_result=-1};
static uint32_t callbacks[17],registration;
static uint32_t configured;   /* 设备级配置(CoD/io cap/0x3d)已下发 */
static void *adapter;
static uint8_t target[6];
static int same(const uint8_t *a,const uint8_t *b)
{for(unsigned i=0;i<6;i++)if(a[i]!=b[i])return 0;return 1;}
/* 回调事件环: 调用者必须已持锁。用于"到底有没有回调、参数是什么"的现场取证。 */
static void event_log(uint32_t kind,const uint8_t *address,uint32_t first,uint32_t second)
{
    if(state.event_head>0x7fffffff)state.event_head=0;
    struct probe_event *slot=&state.history[state.event_head&15];
    if(address)memcpy(slot->address,address,6);else memset(slot->address,0,6);
    slot->kind=kind;slot->first=first;slot->second=second;state.event_head++;
}
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
{(void)cookie;uint32_t p=lock();state.adapter_state=(uint32_t)value;event_log('A',0,(uint32_t)value,0);unlock(p);}
static void on_scan(void *cookie,int value)
{
    (void)cookie;uint32_t p=lock();state.discovery_callbacks++;state.discovery_value=(uint32_t)value;
    event_log('S',0,(uint32_t)value,0);
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
    event_log('P',address,0,0);
    if((state.phase==PAIR || state.phase==CONNECTING) && !state.abort && same(address,state.bond_addr))state.pair_reply=1;
    unlock(p);
}
static void on_display(void *cookie,const uint8_t *address,int transport,int kind,uint32_t passkey)
{
    (void)cookie;if(!address)return;
    uint32_t p=lock();
    event_log('D',address,(uint32_t)transport,(((uint32_t)(uint16_t)kind)<<24)|(passkey&0xffffff));
    /* 043 的回调 transport 语义没有设备证据(参考实现来自 q66 188)。不按 transport
     * 过滤, 只按地址匹配 —— 过滤错了会静默丢事件, 表现为"永远收不到配对码"。 */
    if((state.phase==PAIR || state.phase==CONNECTING) && !state.abort && same(address,state.bond_addr)) {
        state.passkey=passkey;state.pair_confirm=1;
    }
    unlock(p);
}
static void on_bond(void *cookie,const uint8_t *address,int transport,int value)
{
    (void)cookie;if(!address)return;
    uint32_t p=lock();
    state.bond_events++;state.bond_last_value=(uint32_t)value;state.bond_last_transport=(uint32_t)transport;
    event_log('B',address,(uint32_t)transport,(uint32_t)value);
    /* 043 的 bond 状态枚举(固件字符串表实证): 0=BOND_NONE 1=BONDING 2=BONDED。
     * 旧代码只认 2, 把 1(正在配对) 当成失败落 -611 —— 新设备一定先收到 1,
     * 于是一点就报 -611; 已配对设备直接回 2 所以能连。 */
    if(state.phase==PAIR && same(address,state.bond_addr)) {
        state.bond=(uint32_t)value;
        if(value==2 && !state.abort) {
            state.connect_ready=1;state.bond_attempts=0;state.bond_pending=0;state.bonding=0;
        } else if(value==1) {
            /* 配对进行中(等对端确认/交换密钥): 保持 PAIR, 不重发, 不判死。 */
            state.bonding=1;state.bond_pending=1;state.bond_retry_at=state.ticks+40;
        } else if(value==0) {
            /* 未绑定: 可能是"重建配对前先清旧密钥"的快照, 也可能真的失败。
             * 交给 bt_probe_poll 节流重试, 超限才 -611; 下一拍先 remove bond(0x5f)
             * 清掉残留绑定再重配 —— 电脑端有旧配对记录时 createbond 会被直接拒。 */
            state.bonding=0;state.bond_pending=0;state.bond_retry_at=state.ticks+20;
            state.bond_remove_pending=1;
        } else if(!state.abort) {
            state.bonding=0;state.phase=IDLE;state.error=-611;state.error_stage=6;
        }
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
    /* 设备级初始化, 照抄系统 btmain 的序列(0xc70B004):
     *   cmd 0x27 CoD=0x280704  →  settable class of device
     *   cmd 0x29 io capability →  系统用 3(NoInputNoOutput/Just Works),
     *                             这里用 1(DisplayYesNo): 对端是 YesNo/KeyboardDisplay
     *                             时会走 Numeric Comparison, 手环能显示配对码
     *                             (参考项目 q66 的 on_display→passkey→确认 流程)。
     *   cmd 0x3d {0,1}         →  系统同值; 此前完全缺失, 配对一直被对端取消。
     * 配对请求仍自动 accept 兜底, 所以即使对端走 Just Works 也一定能完成。 */
    if(!configured){
        uint32_t cod=0x00280704u;int rc_cod=request(0x27,&cod,4);
        uint8_t capability[1]={1};int rc_cap=request(0x29,capability,1);
        uint8_t pairing[2]={0,1};int rc_pair=request(0x3d,pairing,2);
        uint32_t q=lock();
        state.result_cod=rc_cod;state.result_iocap=rc_cap;state.result_paircfg=rc_pair;
        state.device_configured=1;unlock(q);
        configured=1;
    }
    /* 本地蓝牙名: cmd 0x22 set / 0x23 get(固件 bttool "name"/"get name")。
     * 每次 prepare 都重下发 —— 系统"设备名称"变更或重启会把 adapter 名顶回
     * 默认值(旧版本连手机显示 Syspatch 就是这么来的), 只设一次会被覆盖。 */
    {
        uint8_t name[64]={0};
        static const char label[]="Chaos-Module";
        for(unsigned i=0;i<sizeof(label)-1 && i<63;i++)name[i]=(uint8_t)label[i];
        int rc_set=request(0x22,name,sizeof(name));
        uint8_t verify[64]={0};
        int rc_get=exchange(0x23,0,0,verify,sizeof(verify));
        uint32_t q=lock();
        state.result_name=rc_set;state.result_name_read=rc_get;
        for(unsigned i=0;i<32;i++)state.local_name[i]=(char)verify[i];
        state.local_name[32]=0;unlock(q);
    }
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
    out->error=state.error;out->error_stage=state.error_stage;
    out->bond_attempts=state.bond_attempts;out->bond_pending=state.bond_pending;
    out->bonding=state.bonding;out->bond_events=state.bond_events;
    out->bond_value=state.bond_last_value;out->bond_transport=state.bond_last_transport;
    out->bond_order=state.bond_order;out->phase=state.phase;
    out->pair_reply_result=state.pair_reply_result;
    out->device_configured=state.device_configured;
    out->result_cod=state.result_cod;out->result_iocap=state.result_iocap;
    out->result_paircfg=state.result_paircfg;out->result_connect=state.result_connect;
    out->result_bond=state.result_bond;out->result_confirm=state.result_confirm;
    out->result_remove=state.result_remove;
    out->result_name=state.result_name;
    out->result_name_read=state.result_name_read;
    for(unsigned i=0;i<32;i++)out->local_name[i]=state.local_name[i];
    unlock(p);
}
_Static_assert(sizeof(struct bt_probe_diagnostics)==164,"Rust diagnostics ABI");
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
        uint8_t payload[8];memcpy(payload,state.bond_addr,6);payload[6]=payload[7]=1;
        error(request(98,payload,sizeof(payload)));return;
    }
    if((state.phase!=IDLE && state.phase!=SCAN) || !state.count){state.error=-607;unlock(p);return;}
    int scanning=state.phase==SCAN;
    memcpy(target,state.devices[state.selected].address,6);state.phase=PAIR;state.abort=0;state.error=0;state.error_stage=0;
    state.started=state.ticks;state.cid=state.mtu=0;state.bond=state.pair_confirm=state.connect_ready=state.pair_reply=0;
    state.bond_attempts=0;state.bond_retry_at=0;state.bond_order=0;memset(state.bond_addr,0,6);
    state.bond_pending=state.bonding=state.bond_events=0;state.bond_last_value=state.bond_last_transport=0xffff;
    state.pair_reply_result=0xffff;
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
    /* 建绑定发起/重试。bond_pending = "已发 94, 正等回调", 期间不重发;
     * BONDING(1) 期间同样不重发(重发会打断对端正在确认的配对)。
     * 回调迟迟不来(超过窗口且没有进入 BONDING)才放开 pending 重试。 */
    if(state.bond_pending && !state.bonding && state.ticks>=state.bond_retry_at)state.bond_pending=0;
    int bond=state.phase==PAIR && state.scan_stopped && !state.abort && !state.bond_pending && state.ticks>=state.bond_retry_at;
    if(bond && state.bond_attempts>=8){state.phase=IDLE;state.error=-611;state.error_stage=6;bond=0;}
    if(bond){state.bond_attempts++;state.bond_retry_at=state.ticks+20;state.bond_pending=1;}
    int connect=state.phase==PAIR && state.connect_ready && !state.abort;
    int pair_reply=(state.phase==PAIR || state.phase==CONNECTING) && state.pair_reply && !state.abort;state.pair_reply=0;
    if(connect){state.phase=CONNECTING;state.connect_ready=0;state.started=state.ticks;}
    /* 配对确认要等对端用户点弹窗, BONDING 期间给足 60s。
     * 超时必须真的退出 PAIR/CONNECTING —— 旧代码只置 error 不改 phase,
     * 于是 network_service 永远看到 bt.phase!=0, 换设备连接直接报 -16(EBUSY)。 */
    if((state.phase==CONNECTING || state.phase==PAIR) && state.ticks-state.started>(state.bonding?240:180)) {
        state.error=-614;state.phase=IDLE;state.bond_pending=state.bonding=0;
    }
    if(state.phase==LINK && state.bnep==BNEP_STATE_WAIT_FOR_CONNECTION_RESPONSE && state.ticks-state.started>40)state.error=-616;
    unlock(p);
    if(pair_reply){
        /* 两种配对确认命令都发:
         *   cmd 0x61 = pair request reply {addr, accept}
         *   cmd 0x62 = confirmation {addr, transport, conform}
         * 不同栈把 Numeric Comparison 的确认放在不同命令上, 只回 0x61 时
         * 对端会一直等确认直到取消(真机现象: bond 1 → 0)。失败无害。 */
        uint8_t accept[7]={0};memcpy(accept,state.bond_addr,6);accept[6]=1;
        int rc=request(97,accept,sizeof(accept));
        uint8_t confirm[8]={0};memcpy(confirm,state.bond_addr,6);confirm[6]=1;confirm[7]=1;
        int rc2=request(98,confirm,sizeof(confirm));
        p=lock();state.pair_reply_result=(uint32_t)rc;state.result_confirm=rc2;unlock(p);
        if(rc)error(rc);
    }
    if(bond){
        /* 043 实测: cmd 92 的回复对新/旧设备不可区分(未绑定也回"已绑定"),
         * 不能作为是否发起配对的依据 —— 之前只有先在电脑/手机上手动配过对的
         * 设备能连上, 就是这个原因。改为连接时总是发起建绑定(94):
         *   新设备  -> 对端弹配对确认, bond=2 回调后走连接;
         *   已绑定  -> 栈静默完成(直接 bond=2), 或 94 被拒(rc!=0)时退回直连。
         * 38/37 只在可用时设置 bondable 模式, 失败不拦路。
         * 地址序: L2CAP connect 用扫描序已实证可用(已配对设备能连), 建绑定默认
         * 同一序; 连续失败 3 次后才试反转序(某些栈的 bond API 要显示序)。 */
        p=lock();
        if(state.bond_attempts>3 && (state.bond_attempts&1)){for(unsigned i=0;i<6;i++)state.bond_addr[i]=target[5-i];state.bond_order=1;}
        else{memcpy(state.bond_addr,target,6);state.bond_order=0;}
        unlock(p);
        int mode=request(38,0,0);
        if(!mode || (mode>0 && mode<=3)){uint8_t settings[2]={(uint8_t)mode,1};request(37,settings,2);}
        /* 上一轮 bond=0(配对失败) 时, 先删掉本机残留绑定再重配: 电脑/手机端
         * 若保留旧配对记录, createbond 会被对方直接拒绝(现象就是 bond 1→0 循环)。
         * cmd 0x5f = remove bond {addr 6B, transport}。 */
        if(state.bond_remove_pending){
            uint8_t remove[7]={0};memcpy(remove,state.bond_addr,6);remove[6]=1;
            int rc_remove=request(0x5f,remove,sizeof(remove));
            p=lock();state.result_remove=rc_remove;state.bond_remove_pending=0;unlock(p);
        }
        /* 经典 PAN 建绑定前要先有 ACL: 股票 bttool 的用法也是先 connect <addr>
         * 再 createbond <addr> 1(transport: 0=BLE 1=BREDR, 见 bttool 帮助文本)。
         * 未连接设备上直接 createbond 会立刻回 bond=0 → -611。
         * 另外先设 IO capability(1=DisplayYesNo), 否则 SSP 可能因能力未定失败。
         * 两条都幂等, 失败不拦路(已连接时 connect 返回"已连接"错误属正常)。 */
        /* IO capability 已在 prepare() 按系统 btmain 序列设成 3(Just Works):
         * 对端不需要我们显示/确认数字, 收到配对请求回 accept 即完成。 */
        int rc_connect=request(0x69,state.bond_addr,6);
        uint8_t payload[8]={0};memcpy(payload,state.bond_addr,6);payload[6]=1;
        int rc=request(94,payload,8);
        p=lock();int ipc=state.ipc_result;state.result_connect=rc_connect;state.result_bond=rc;unlock(p);
        if((rc && rc!=-11) || ipc){p=lock();state.bond=2;state.bonding=0;state.bond_pending=0;state.connect_ready=1;unlock(p);}
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
uint32_t bt_probe_event_text(char *out,uint32_t capacity)
{
    if(!out || !capacity)return 0;
    /* 只拷事件环(320B), 不整份拷贝 state —— 这个函数跑在 20KB 任务栈上。 */
    struct probe_event events[16];uint32_t reference;
    uint32_t p=lock();memcpy(events,state.history,sizeof(events));reference=state.event_head;unlock(p);
    struct output o={out,0,capacity};
    uint32_t total=reference,count=total<12u?total:12u;
    if(!count)str(&o,"暂无回调事件");
    for(uint32_t i=0;i<count;i++){
        struct probe_event *e=&events[(total-count+i)&15];
        if(i)chr(&o,'\n');
        const char *n="?";
        switch(e->kind){case 'A':n="adapter";break;case 'S':n="scan";break;case 'V':n="device";break;
            case 'P':n="pair";break;case 'D':n="display";break;case 'B':n="bond";break;case 'L':n="l2";break;}
        (void)0;str(&o,n);chr(&o,' ');
        unsigned any=0;for(unsigned b=0;b<6;b++)if(e->address[b])any=1;
        if(any){for(unsigned b=0;b<6;b++){if(b)chr(&o,':');num(&o,e->address[5-b],16,2);}}
        else str(&o,"--");
        str(&o,"  a=");if((int32_t)e->first<0)chr(&o,'-');num(&o,(int32_t)e->first<0?0u-e->first:e->first,10,1);
        str(&o," b=");if((int32_t)e->second<0)chr(&o,'-');num(&o,(int32_t)e->second<0?0u-e->second:e->second,10,1);
    }
    if(capacity)out[o.n]=0;
    return o.n;
}
