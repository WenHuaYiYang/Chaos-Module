#include "network_service.h"
#include "pan_ip.h"
#include "pan_radio.h"
#include <string.h>
#define COMMAND_QUEUE 8u
static struct network_view state={.enabled=0},published={.enabled=0};
static unsigned commands[COMMAND_QUEUE],command_indices[COMMAND_QUEUE];
static unsigned command_head,command_tail,command_count;
static unsigned queue_overflow;
/* Last switch target is independent of the action ring. Disconnect cancels
 * actions, not the user's pending ON/OFF choice. Protected by lock(). */
static unsigned requested_enabled;
static unsigned discovering,next_peer,active_query,auto_cid,connecting,link_ticks,sdp_retries;
static unsigned pending_connection;
static unsigned scan_when_idle;
/* 记住的对端地址: 只在 UI 线程写、worker 取走即清, 用 lock() 保护。 */
static uint8_t pending_address[6];
static char pending_name[21];
static unsigned pending_valid;
static int last_error;
static unsigned lock(void){unsigned p;__asm__ volatile("mrs %0, primask\ncpsid i":"=r"(p)::"memory");return p;}
static void unlock(unsigned p){__asm__ volatile("msr primask, %0"::"r"(p):"memory");}
static void request(unsigned c,unsigned i){unsigned p=lock();
    if(c==5)requested_enabled=i!=0;
    if(c==3)i=requested_enabled;
    /* Stop/disable supersedes outstanding actions even when UI input filled
     * the queue. Never execute an obsolete connect after the user stopped. */
    if(c==3 || (c==5 && !i)){command_head=command_tail=command_count=queue_overflow=0;pending_valid=0;}
    for(unsigned n=0;n<command_count;n++){unsigned at=(command_head+n)%COMMAND_QUEUE;
        if(commands[at]==c && (c!=5 || command_indices[at]==i)){command_indices[at]=i;unlock(p);return;}}
    if(command_count<COMMAND_QUEUE){commands[command_tail]=c;command_indices[command_tail]=i;command_tail=(command_tail+1)%COMMAND_QUEUE;command_count++;}
    else queue_overflow=1;
    unlock(p);}
void network_scan(void){request(1,0);}
void network_connect(unsigned i){request(2,i);}
void network_disconnect(void){request(3,0);}
void network_pair_confirm(void){request(4,0);}
void network_enable(int enabled){request(5,enabled!=0);}
/* 记住的对端直连: 只投递地址, 真正的 BT 操作仍在 worker 里做。
 * 拒绝条件与 c==2 一致(扫描/连接进行中), 且要求已启用。 */
int network_connect_address(const uint8_t *address,const char *name)
{
    if(!address || !name)return -22;
    unsigned p=lock();
    int busy=!requested_enabled || discovering || connecting || pending_connection;
    if(!busy){memcpy(pending_address,address,6);memset(pending_name,0,sizeof(pending_name));
        for(unsigned i=0;i<20 && name[i];i++)pending_name[i]=name[i];pending_valid=1;}
    unlock(p);
    if(busy)return -16;
    request(6,0);
    return 0;
}
void network_service_view(struct network_view *out){if(out){unsigned p=lock();memcpy(out,&published,sizeof(*out));unlock(p);
    /* A quarantined PAN worker no longer polls/publishes. Report the atomic
     * fault from the UI reader instead of showing stale NETWORK_READY. */
    if(pan_ip_faulted()){out->phase=NETWORK_IDLE;out->error=-5;out->error_stage=7;out->error_serial++;}}}
static void error(int e,unsigned stage){if(e && e!=last_error){state.error=e;state.error_stage=stage;state.error_serial++;}last_error=e;}
void network_service_poll(void)
{
    unsigned p=lock();unsigned c=0,index=0,overflow=queue_overflow;queue_overflow=0;
    if(command_count){c=commands[command_head];index=command_indices[command_head];command_head=(command_head+1)%COMMAND_QUEUE;command_count--;}
    unlock(p);
    struct bt_probe_view bt;bt_probe_view(&bt);network_sdp_poll();
    if(overflow)error(-16,1);
    if(c==3){
        state.enabled=index!=0;connecting=discovering=active_query=scan_when_idle=0;
        auto_cid=pending_connection=0;sdp_retries=0;network_sdp_cancel();bt_probe_disconnect();
        if(!state.enabled)state.count=0;
        c=0;
    }
    if(c==5){unsigned changed=state.enabled!=(index!=0);state.enabled=index!=0;
        if(changed){state.error=last_error=0;pending_connection=0;}
        if(!state.enabled){discovering=active_query=connecting=scan_when_idle=0;sdp_retries=0;state.count=0;network_sdp_cancel();bt_probe_disconnect();}
        else if(changed && !bt.cid)scan_when_idle=1;
        c=0;
    }
    if(c==6){
        uint8_t address[6];char name[21];
        p=lock();memcpy(address,pending_address,6);memcpy(name,pending_name,21);unsigned valid=pending_valid;pending_valid=0;unlock(p);
        if(valid){
            if(state.enabled && !discovering && !connecting && !bt.phase && !bt.cid){
                if(!bt_probe_connect_address(address,name)){
                    memcpy(state.address,address,6);memset(state.connected_name,0,sizeof(state.connected_name));memcpy(state.connected_name,name,21);
                    state.error=last_error=0;auto_cid=0;connecting=1;link_ticks=0;
                } else error(-16,2);
            } else error(-16,2);
        }
    }
    if(!state.enabled)c=0;
    if(state.enabled && scan_when_idle && bt.phase==0 && !bt.cid){scan_when_idle=0;c=1;}
    if(c==1){
        if(discovering || bt.phase==1){/* Repeated scan while already scanning is idempotent. */}
        else if(bt.phase || connecting){error(-16,1);}
        else {state.count=state.checked=state.candidates=0;next_peer=active_query=0;pending_connection=0;sdp_retries=0;state.error=last_error=0;discovering=1;bt_probe_scan();}
    }else if(c==2){
        if(discovering && index<state.count){pending_connection=index+1;}
        else if(connecting || bt.phase || index>=state.count)error(-16,2);
        else {struct network_peer *peer=&state.peers[index];state.selected=index;memcpy(state.address,peer->device.address,6);memcpy(state.connected_name,peer->device.name,21);state.connected_name[21]=0;
            state.error=last_error=0;auto_cid=0;connecting=1;link_ticks=0;bt_probe_connect_device(peer->probe_index);}
    }else if(c==4 && bt.pair_confirm)bt_probe_connect();
    bt_probe_view(&bt);
    if(discovering && bt.phase!=1){
        if(bt.error){error(bt.error,1);discovering=0;}
        else {
            state.candidates=bt.count;
            if(active_query){int rc=network_sdp_result();if(rc!=-11){
                if(rc==1 && state.count<16){struct network_peer *peer=&state.peers[state.count];if(bt_probe_device(next_peer,&peer->device)){peer->probe_index=next_peer;state.count++;}}
                /* One peer's timeout/absence is not a whole scan failure. */
                if(rc<0 && rc!=-110)error(rc,3);next_peer++;state.checked=next_peer;active_query=0;
            }}
            if(!active_query){if(pending_connection){unsigned target=pending_connection-1;pending_connection=0;discovering=0;request(2,target);}
                else if(next_peer<bt.count){struct bt_probe_device d;if(bt_probe_device(next_peer,&d)){int rc=network_sdp_start(d.address);if(!rc){active_query=1;sdp_retries=0;}else if(rc==-16){/* previous SDP still closing; wait a few polls, then skip this peer */if(++sdp_retries>24){sdp_retries=0;next_peer++;state.checked=next_peer;}}else {error(rc,3);discovering=0;}}else next_peer++;}
                else discovering=0;}
        }
    }
    if(state.enabled && (connecting || bt.cid) && bt.error){error(bt.error,bt.cid && (bt.error==-616 || bt.error<=-620)?5:4);connecting=0;bt_probe_disconnect();}
    if(state.enabled && bt.phase==4 && bt.cid){
        if(!bt.bnep && auto_cid!=bt.cid){auto_cid=bt.cid;if(bt.mtu<1691)error(-90,5);else bt_probe_bnep();}
        if(bt.bnep==3)connecting=0;
        if(++link_ticks>240 && bt.bnep==3 && !pan_ip_ready()){
            /* 地址不全时 pan_ip_poll 的 link 条件不成立, DHCP 一个包都没发过,
             * 报 -110 会把它误导成"手机没给 IP"。分开报。 */
            uint8_t local_address[6],remote_address[6];
            error(pan_radio_addresses(local_address,remote_address)?-110:-97,6);
            connecting=0;bt_probe_disconnect();
        }
    }else if(!bt.cid){auto_cid=0;link_ticks=0;}
    state.passkey=bt.passkey;state.cid=bt.cid;state.mtu=bt.mtu;
    state.phase=!state.enabled?NETWORK_IDLE:bt.phase==1?NETWORK_SCANNING:discovering?NETWORK_DISCOVERING:bt.pair_confirm?NETWORK_PAIRING:
        bt.phase==2 || bt.phase==3?NETWORK_CONNECTING:bt.phase==4?(bt.bnep==3?(pan_ip_ready()?NETWORK_READY:NETWORK_DHCP):NETWORK_NEGOTIATING):NETWORK_IDLE;
    p=lock();memcpy(&published,&state,sizeof(state));unlock(p);
}
