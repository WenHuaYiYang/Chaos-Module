/* Exact p67 3.101.043 L2CAP ABI: see docs/BTPAN-ABI.md.
 * Own callback, buffers, CID and transaction; never tears down the PAN CID. */
#include "network_service.h"
#include "nap_sdp.h"
#include "bt_addresses.h"
#include <string.h>
#define FN(n,t) ((t)(uintptr_t)(n))
static unsigned lock(void){unsigned p;__asm__ volatile("mrs %0, primask\ncpsid i":"=r"(p)::"memory");return p;}
static void unlock(unsigned p){__asm__ volatile("msr primask, %0"::"r"(p):"memory");}
static uint16_t le16(const uint8_t *p){return (uint16_t)(p[0]|p[1]<<8);}
static void put16(uint8_t *p,unsigned v){p[0]=(uint8_t)v;p[1]=(uint8_t)(v>>8);}
static void *alloc(unsigned n){return FN(BT_ALLOC,void*(*)(unsigned))(n);}
static void release(void *p){FN(BT_FREE,void(*)(void*))(p);}
struct work {unsigned op;uint16_t cid;unsigned length;uint8_t address[6],data[32];};
static struct nap_sdp parser;
static uint16_t cid,mtu;
static unsigned busy,connecting,closing,ticks,opened,packet_length;
static uint8_t packet[1691];
static int result=-11,completed,transport_error;
static int event(uint32_t,void *);
static void fail(int error){unsigned p=lock();if(!completed){result=error;completed=1;}unlock(p);}
static int cancel(int v,int e,void *a){(void)v;(void)e;struct work *w=a;unsigned p=lock();if(w->op==1)connecting=0;if(w->op==2)closing=0;transport_error=-125;unlock(p);release(w);return 0;}
static int run(int v,int e,void *a)
{
    (void)v;(void)e;struct work *w=a;unsigned p=lock();unsigned current=busy;uint16_t owned=cid;unsigned done=completed;unlock(p);
    if(w->op==1 && current && !done){
        uint8_t *r=alloc(68);if(!r){cancel(0,0,a);return 0;}memset(r,0,68);
        put16(r+2,1);uint32_t cb=(uint32_t)(uintptr_t)event;memcpy(r+12,&cb,4);memcpy(r+16,w->address,6);put16(r+52,1691);put16(r+54,1);
        if(!FN(BT_L2CAP_CONNECT,unsigned(*)(const void*))(r)){release(r);p=lock();connecting=0;transport_error=-111;unlock(p);}
    }else if(w->op==2 && owned==w->cid && owned>0x3f){
        uint8_t *r=alloc(4);if(!r){cancel(0,0,a);return 0;}put16(r,owned);put16(r+2,0);
        if(!FN(BT_L2CAP_DISCONNECT,int(*)(const void*))(r)){release(r);p=lock();closing=0;transport_error=-5;unlock(p);}
    }else if(w->op==3 && current && !done && owned==w->cid && owned>0x3f){
        unsigned total=w->length+16;uint8_t *r=alloc(total+6);if(!r){cancel(0,0,a);return 0;}
        memset(r,0,total+6);put16(r,total);put16(r+2,16);r[8]=1;memcpy(r+20,w->data,w->length);
        if(!FN(BT_L2CAP_SUBMIT_CID,int(*)(void*,uint16_t))(r,owned)){release(r);p=lock();transport_error=-5;unlock(p);}
    }else if(w->op==1){p=lock();connecting=0;unlock(p);}
    release(w);return 0;
}
static int queue(unsigned op,const uint8_t *data,unsigned n)
{
    void *owner=*(void *volatile*)(uintptr_t)BT_OWNER_SLOT;if(!owner)return -19;
    if(n>32)return -90;struct work *w=alloc(sizeof(*w));if(!w)return -12;memset(w,0,sizeof(*w));w->op=op;
    unsigned p=lock();w->cid=cid;unlock(p);w->length=n;
    if(op==1)memcpy(w->address,data,6);else if(n)memcpy(w->data,data,n);
    FN(BT_QUEUE_EXTERNAL,int(*)(void*,int(*)(int,int,void*),int(*)(int,int,void*),void*,uint8_t))(owner,run,cancel,w,0x71);
    return 0;
}
static int event(uint32_t e,void *arg)
{
    if(!arg)return 0;const uint8_t *a=arg;unsigned p=lock();
    if(e==2){uint16_t c=le16(a);if(connecting && !cid && c>0x3f){cid=c;connecting=0;}else {if(c<=0x3f)connecting=0;transport_error=-111;}}
    else if(e==3){if(cid>0x3f && le16(a+108)==cid){mtu=le16(a+72);opened=1;}else transport_error=-71;}
    else if(e==6){if(le16(a)==cid){cid=mtu=0;connecting=closing=0;if(!completed)transport_error=-104;}}
    else if(e==7){unsigned total=le16(a),offset=le16(a+2);
        if(cid>0x3f && le16(a+4)==cid && offset>=6 && offset<=64 && total>=offset && total-offset<=sizeof(packet) && !packet_length){packet_length=total-offset;memcpy(packet,a+4+offset,packet_length);}
        else transport_error=-71;
    }
    unlock(p);release(arg);return 0;
}
int network_sdp_start(const uint8_t address[6])
{
    unsigned p=lock();if(busy || cid || connecting){unlock(p);return -16;}
    busy=connecting=1;closing=opened=packet_length=ticks=0;transport_error=completed=0;result=-11;unlock(p);
    nap_sdp_init(&parser);int rc=queue(1,address,6);if(rc){p=lock();connecting=0;unlock(p);fail(rc);}return 0;
}
void network_sdp_poll(void)
{
    static uint8_t incoming[1691];unsigned p=lock();if(!busy){unlock(p);return;}
    unsigned open=opened;opened=0;unsigned n=packet_length;if(n){memcpy(incoming,packet,n);packet_length=0;}
    int error=transport_error;transport_error=0;unsigned done=completed;uint16_t c=cid,m=mtu;unsigned pending=connecting;unsigned close=closing;ticks++;unlock(p);
    if(!done){
        if(error)fail(error);
        else if(ticks>40)fail(-110);
        else {
            int send=open;
            if(n){int rc=nap_sdp_response(&parser,incoming,n);if(rc<0)fail(rc);else if(rc==1){p=lock();completed=1;result=parser.count?1:0;unlock(p);}else send=1;}
            p=lock();done=completed;unlock(p);
            if(send && !done){uint8_t out[32];int length=nap_sdp_request(&parser,out,sizeof(out));if(length<0)fail(length);else if(m<(unsigned)length)fail(-90);else {int rc=queue(3,out,(unsigned)length);if(rc)fail(rc);}}
        }
    }
    p=lock();done=completed;unlock(p);
    if(done && c>0x3f && !close){p=lock();closing=1;unlock(p);int rc=queue(2,0,0);if(rc){p=lock();closing=0;unlock(p);}}
    /* A timed-out connect retains its callback/CID ownership until its late
     * callback arrives. Never repurpose a live/pending channel for another peer. */
    if(done && !c && !pending){p=lock();busy=0;unlock(p);}
}
int network_sdp_result(void)
{unsigned p=lock();int r=busy && !(completed && ticks>48)?-11:result;unlock(p);return r;}
void network_sdp_cancel(void){fail(-125);}

