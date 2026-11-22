/* ChaosConnect p67/3.101.043 transports. Private authenticated Xiaomi PB
 * traffic is consumed and other namespaces pass through unchanged. Android
 * adds a dedicated standard SPP/RFCOMM carrier, sharing the CCN1 owner queue.
 * Pair/service IPC runs in an isolated task; VFS on owner; stream I/O on BT. */
#include "chaos_connect.h"
#include <stddef.h>
#include <stdint.h>
#define L2_RX 0x0c7d6e6du
#define L2_SEND 0x0c7d719du
#define L2_SESSION 0x0c7d7e95u
#define L2_CRYPTO 0x0c7d46a5u
#define L2_CONNECTIONS 0x200e16b8u
#define L2_TIMER 0x0c587ed1u
#define L2_TIMER_DELETE 0x0c588129u
#define OWNER_LOOP 0x20125700u
#define BT_LOOP 0x201259e4u
#define TIMER_INIT 0x0c2e3995u
#define TIMER_START 0x0c2e3b85u
#define TIMER_SIZE 136u
#define RX_SLOTS (CC_WINDOW+4u)
#define TX_SLOTS (CC_WINDOW*2u)
#define FN(a,t) ((t)(uintptr_t)(a))
struct l2_packet {uint8_t channel,opcode;uint16_t reserved;void *payload;uint16_t len,padding;void *complete,*complete_cookie,*sent,*sent_cookie;};
_Static_assert(sizeof(struct l2_packet)==28,"p67 L2 packet ABI");
typedef void (*rx_fn)(uint16_t,uint8_t,uint8_t,void *,uint16_t);
typedef void *(*session_fn)(uint16_t);
typedef int (*crypto_fn)(void *,int,uint16_t,const void *,void *);
typedef int (*send_fn)(void *,const struct l2_packet *);
typedef int (*init_fn)(void *,void *);
typedef int (*timer_start_fn)(void *,void (*)(void *),uint64_t,uint64_t);
struct request {volatile uint32_t state;uint32_t epoch,id;uint16_t handle,len;uint32_t android;void *session;uint8_t data[CC_MAX_WIRE];};
static struct request rx[RX_SLOTS],tx[TX_SLOTS];
static uint8_t probe[CC_MAX_WIRE],plain[CC_MAX_FRAME],send_copy[CC_MAX_WIRE];
static volatile uint32_t rx_read,rx_write,tx_read,tx_write,epoch,started,reset_pending,wanted;
/* 2026-11-22 功耗: 原先空闲每 20ms 轮询(50Hz), 与维护节拍无关, 省电模式对它无效。
 * 空闲 100ms(省电档 200ms), 有请求待处理仍回到 2ms; 连接列表扫描 250->750ms。 */
static uint32_t bridge_power_saver;
void chaos_bridge_set_power_saver(uint32_t on){bridge_power_saver=on?1u:0u;}
static volatile uint32_t notice_pending,notice_attempts;
static uint16_t notice_handle;
static void *notice_session;
static uint32_t notice_prefix;
static uint8_t notice_wire[128];
static uint16_t handle;
static void *session;
static uint32_t packet_id,request_epoch,prefix,client_android;
static uint32_t owner_init,bt_init,owner_period,bt_period;
static uint32_t attach_last,attach_epoch=~0u;
static uint64_t owner_timer[(TIMER_SIZE+7)/8],bt_timer[(TIMER_SIZE+7)/8];
static void *boot_timer;
static uint32_t lock(void){uint32_t p;__asm__ volatile("mrs %0, primask\n\tcpsid i":"=r"(p)::"memory");return p;}
static void unlock(uint32_t p){__asm__ volatile("msr primask, %0"::"r"(p):"memory");}
static void barrier(void){__asm__ volatile("dmb":::"memory");}
static int ptr(uint32_t p){return !(p&3)&&((p>=0x20000000&&p<=0x2400ffff)||(p>=0x30000000&&p<=0x3fffffff));}
static void copy(void *d,const void *s,uint32_t n){uint8_t *a=d;const uint8_t *b=s;for(uint32_t i=0;i<n;i++)a[i]=b[i];}
uint32_t chaos_bridge_enabled(void){return wanted&&started;}
static void original(uint16_t h,uint8_t ch,uint8_t op,void *p,uint16_t n){FN(L2_RX,rx_fn)(h,ch,op,p,n);}
/* UI-safe gate: no timer, LVGL, VFS, or radio calls from the switch callback. */
void chaos_bridge_set_enabled(uint32_t enabled){
    uint32_t mask=lock();enabled=enabled!=0;
    if(wanted!=enabled){
        if(!enabled&&session&&!client_android){notice_handle=handle;notice_session=session;notice_prefix=prefix;notice_attempts=0;notice_pending=1;}
        wanted=enabled;started=0;epoch++;reset_pending=1;barrier();
    }
    unlock(mask);
}
static void receive(uint16_t h,uint8_t ch,uint8_t op,void *p,uint16_t n){
    if(!wanted||!started||ch!=1||op!=2||!p||n>CC_MAX_WIRE){original(h,ch,op,p,n);return;}
    uint32_t received_epoch=epoch;
    void *s=FN(L2_SESSION,session_fn)(h);const uint8_t *frame;uint32_t size,id;
    /* 0x0c7025e8 copies the IV to its stack: probing does not advance the
     * session cipher. Unrelated encrypted traffic is forwarded byte-for-byte. */
    if(!s||FN(L2_CRYPTO,crypto_fn)(s,0,n,p,probe)!=0||cc_pb_unwrap(probe,n,&frame,&size,&id)!=1){original(h,ch,op,p,n);return;}
    struct cc_frame f;if(cc_decode(frame,size,&f)||f.flags||f.status||f.req!=(id&0xffffu))return;
    uint32_t mask=lock(),slot=rx_write;
    if(!wanted||!started||received_epoch!=epoch){unlock(mask);return;}
    if(rx[slot].state){unlock(mask);return;} /* Peer is bounded by HELLO's window. */
    rx[slot].state=1;rx[slot].epoch=epoch;rx_write=(slot+1)%RX_SLOTS;unlock(mask);
    rx[slot].android=0;rx[slot].handle=h;rx[slot].session=s;rx[slot].id=id;rx[slot].len=size;copy(rx[slot].data,frame,size);barrier();rx[slot].state=2;
}

static int chaos_bridge_android_receive(uint32_t generation,const uint8_t *data,unsigned length){
    uint32_t received_epoch=epoch;
    const uint8_t *frame;uint32_t size,id;
    if(!wanted||!started||cc_pb_unwrap(data,length,&frame,&size,&id)!=1)return 0;
    struct cc_frame f;if(cc_decode(frame,size,&f)||f.flags||f.status||f.req!=(id&0xffffu))return 0;
    uint32_t mask=lock(),slot=rx_write;
    if(!wanted||!started||received_epoch!=epoch||rx[slot].state){unlock(mask);return 0;}
    rx[slot].state=1;rx[slot].epoch=epoch;rx_write=(slot+1)%RX_SLOTS;unlock(mask);
    rx[slot].android=1;rx[slot].handle=0;rx[slot].session=(void *)(uintptr_t)generation;
    rx[slot].id=id;rx[slot].len=size;copy(rx[slot].data,frame,size);barrier();rx[slot].state=2;return 1;
}
#include "android_radio.inc"
static int live_session(unsigned android,uint16_t h,void *s){return android?cc_radio_live(s):FN(L2_SESSION,session_fn)(h)==s;}
static void attach(int remove){
    uint32_t mask=lock();volatile uint32_t *head=(void *)(uintptr_t)L2_CONNECTIONS;
    uint32_t node=head[1];
    for(unsigned i=0;i<64&&ptr(node)&&node!=L2_CONNECTIONS;i++){
        volatile uint32_t *list=(void *)(uintptr_t)node;uint32_t c=list[3];
        if(ptr(c)){volatile uint32_t *conn=(void *)(uintptr_t)c;
            uint32_t want=remove?(uint32_t)(uintptr_t)receive:L2_RX;
            if(conn[3]==want)conn[3]=remove?L2_RX:(uint32_t)(uintptr_t)receive;
        }node=list[1];
    }unlock(mask);
}
static uint16_t native_chunk(uint16_t h){
    volatile uint32_t *head=(void *)(uintptr_t)L2_CONNECTIONS;uint32_t node=head[1];
    for(unsigned i=0;i<64&&ptr(node)&&node!=L2_CONNECTIONS;i++){
        volatile uint32_t *list=(void *)(uintptr_t)node;uint32_t c=list[3];
        if(ptr(c)&&*(volatile uint16_t *)(uintptr_t)(c+6)==h){
            uint32_t mtu=*(volatile uint16_t *)(uintptr_t)(c+0x52);
            /* SAR submit rejects len+1 >= negotiated mtu. Leave envelope/headroom. */
            if(mtu<=256)return 0;uint32_t n=mtu-128;return n>CC_CHUNK?CC_CHUNK:(uint16_t)n;
        }node=list[1];
    }return 0;
}
static unsigned tx_free(void){unsigned n=0;for(unsigned i=0;i<TX_SLOTS;i++)if(tx[i].state==0)n++;return n;}
int cc_reply(uint8_t flags,uint8_t op,uint8_t status,uint16_t req,const void *data,uint32_t n){
    if(!wanted||!started||request_epoch!=epoch||!session)return -1;
    int len=cc_encode(plain,sizeof(plain),flags,op,status,req,data,n);if(len<0)return -1;
    uint32_t mask=lock(),slot=tx_write;
    if(!wanted||!started||request_epoch!=epoch){unlock(mask);return -1;}
    if(tx[slot].state){unlock(mask);return -1;}tx[slot].state=1;tx_write=(slot+1)%TX_SLOTS;unlock(mask);
    int wire=cc_pb_wrap(tx[slot].data,sizeof(tx[slot].data),plain,len,flags==CC_EVENT?(packet_id&0xffff0000u):packet_id);
    tx[slot].android=client_android;tx[slot].handle=handle;tx[slot].session=session;tx[slot].epoch=request_epoch;tx[slot].len=wire>0?wire:0;
    barrier();tx[slot].state=2;return wire>0?0:-1;
}
static int bt_start(uint32_t period);
static void send_disabled_notice(void){
    if(!notice_pending)return;
    uint32_t mask=lock();uint16_t h=notice_handle;void *s=notice_session;uint32_t id=notice_prefix;unlock(mask);
    if(FN(L2_SESSION,session_fn)(h)!=s){notice_pending=0;return;}
    extern uint32_t chaos_connect_policy(void);
    uint8_t payload[8]={0,0,(uint8_t)chaos_connect_policy(),0,0,0,0,0},frame[24];
    int n=cc_encode(frame,sizeof(frame),CC_EVENT,0xc2,0,0,payload,sizeof(payload));
    int wire=cc_pb_wrap(notice_wire,sizeof(notice_wire),frame,n,id);
    if(wire>0){struct l2_packet p={0};p.channel=1;p.opcode=2;p.payload=notice_wire;p.len=wire;
        if(FN(L2_SEND,send_fn)(s,&p)==0){mask=lock();if(notice_session==s&&notice_prefix==id)notice_pending=0;unlock(mask);}
        else if(++notice_attempts>=3)notice_pending=0;
    }
}
static void bt_tick(void *unused){
    (void)unused;cc_radio_tick(wanted&&started);send_disabled_notice();
    if(!wanted||!started){(void)bt_start(notice_pending?10:1000);return;}
    for(unsigned i=0;i<TX_SLOTS;i++){
        if(!wanted||!started)break;
        uint32_t slot=tx_read;if(tx[slot].state!=2)break;
        if(tx[slot].len&&tx[slot].epoch==epoch&&live_session(tx[slot].android,tx[slot].handle,tx[slot].session)){
            struct l2_packet p={0};p.channel=1;p.opcode=2;p.len=tx[slot].len;p.payload=send_copy;
            copy(send_copy,tx[slot].data,p.len);
            /* Native send encrypts in place and synchronously copies into SAR;
             * retry from immutable plaintext, never encrypt a queued slot twice. */
            if(tx[slot].android){if(cc_radio_send((uint32_t)(uintptr_t)tx[slot].session,send_copy,p.len)!=0)break;}
            else if(FN(L2_SEND,send_fn)(tx[slot].session,&p)!=0)break;
        }barrier();tx[slot].state=0;tx_read=(slot+1)%TX_SLOTS;
    }(void)bt_start(tx[tx_read].state==2?2:(radio_pair_until?100:(radio_path_pending||radio_close_request?20:1000)));
}
static int bt_start(uint32_t period){
    void *loop=*(void *volatile *)(uintptr_t)BT_LOOP;if(!loop)return -1;if(bt_period==period)return 0;
    if(!bt_init){if(FN(TIMER_INIT,init_fn)(loop,bt_timer))return -1;bt_init=1;}
    if(FN(TIMER_START,timer_start_fn)(bt_timer,bt_tick,period,period))return -1;bt_period=period;return 0;
}
static void poll(void){
    cc_radio_log_owner();
    if(reset_pending){cc_files_reset();reset_pending=0;session=0;prefix=0;}
    uint32_t now=chaos_connect_now_ms();
    /* A connection-list scan masks interrupts. Discover new legacy links at
     * 4 Hz, with an immediate pass for every enable/disable epoch. */
    uint32_t scan=bridge_power_saver?1500u:750u;
    if(attach_epoch!=epoch || now-attach_last>=scan){
        attach_epoch=epoch;attach_last=now;attach(!wanted||!started);
    }
    if(!wanted||!started)return;if(!bt_init)(void)bt_start(20);
    if(session&&!live_session(client_android,handle,session)){cc_files_reset();session=0;prefix=0;}
    extern uint32_t chaos_connect_now_ms(void);
    if(cc_files_tick(chaos_connect_now_ms())){session=0;prefix=0;}
    for(unsigned served=0;served<2u&&tx_free()>=3;served++){
        if(!wanted||!started)break;
        uint32_t slot=rx_read;if(rx[slot].state!=2)break;
        struct request *r=&rx[slot];struct cc_frame f;
        if(wanted&&started&&r->epoch==epoch&&r->len&&live_session(r->android,r->handle,r->session)&&cc_decode(r->data,r->len,&f)==0){
            int fresh=client_android!=r->android||session!=r->session||handle!=r->handle||prefix!=(r->id&0xffff0000u);
            if(!fresh||f.op==1){
                if(fresh){cc_files_reset();client_android=r->android;cc_files_set_android(client_android);session=r->session;handle=r->handle;prefix=r->id&0xffff0000u;}
                request_epoch=r->epoch;packet_id=r->id;
                uint16_t chunk=client_android?CC_CHUNK:native_chunk(handle);
                if(chunk){cc_files_set_chunk(chunk);cc_files_dispatch(&f);}
                else (void)cc_reply(CC_RESPONSE,f.op,12,f.req,0,0);
            }
        }barrier();r->state=0;rx_read=(slot+1)%RX_SLOTS;
    }
    if(tx[tx_read].state==2)(void)bt_start(2);
}
/* 2026-11-22 功耗: 原先空闲时每 20ms 轮询一次(50Hz), 与维护节拍无关 ——
 * 静置时省电模式对它完全无效。空闲降到 100ms(省电档 200ms); 一旦有请求
 * 待处理立刻回到 2ms, 传输速度不受影响。 */
static int owner_start(uint32_t period);
static void owner_tick(void *unused){
    (void)unused;poll();
    uint32_t idle=bridge_power_saver?200u:100u;
    (void)owner_start(!started?1000u:(rx[rx_read].state==2?2u:idle));
}
static int owner_start(uint32_t period){
    void *loop=*(void *volatile *)(uintptr_t)OWNER_LOOP;if(!loop)return -1;if(owner_period==period)return 0;
    if(!owner_init){if(FN(TIMER_INIT,init_fn)(loop,owner_timer))return -1;owner_init=1;}
    if(FN(TIMER_START,timer_start_fn)(owner_timer,owner_tick,period,period))return -1;owner_period=period;return 0;
}
static void boot_tick(void *unused){
    (void)unused;
    if(wanted&&owner_start(2)==0)started=1;
    if((started||!wanted)&&boot_timer){FN(L2_TIMER_DELETE,void (*)(void *))(boot_timer);boot_timer=0;}
}
int chaos_bridge_start(void){
    if(!wanted)return -1;
    if(started||boot_timer)return 0;
    if(owner_init){if(owner_start(2))return -1;started=1;if(bt_init)(void)bt_start(10);return 0;}
    boot_timer=FN(L2_TIMER,void *(*)(void (*)(void *),uint32_t,void *))(boot_tick,10,0);
    return boot_timer?0:-1;
}
void chaos_bridge_stop(void){
    chaos_bridge_set_enabled(0);
    started=0;barrier();attach(1);
    if(boot_timer){FN(L2_TIMER_DELETE,void (*)(void *))(boot_timer);boot_timer=0;}
    uint32_t mask=lock();epoch++;reset_pending=1;
    /* Do not overwrite buffers while their owner is processing them. Epochs
     * discard old work; owner/BT drains retire occupied slots in order. */
    unlock(mask);
    if(owner_init)(void)owner_start(10);if(bt_init)(void)bt_start(notice_pending?10:1000);
}
