#include "pan_ethernet.h"
#include <string.h>
/* Module-private lwIP transport, not a system net_driver_s or socket device.
 * Never pass a partial/guessed kernel net_driver_s to a firmware function. */
extern int hal_bt_l2cap_send(const uint8_t *,uint16_t);
struct packet {unsigned length;uint8_t data[PAN_ETH_FRAME_MAX];};
struct ring {unsigned head,count;struct packet packets[PAN_ETH_QUEUE_SIZE];};
static struct ring rx,tx;
static struct pan_eth_backend backend;
static void *native;
static struct pan_eth_stats stats;
static unsigned generation,rx_running,tx_running;
/* Separate persistent scratch buffers avoid adding 1.5KB to native task stacks. */
static struct packet rx_work,tx_work;
static uint8_t bnep_work[PAN_ETH_FRAME_MAX+1];
static unsigned lock(void)
{unsigned p;__asm__ volatile("mrs %0, primask\ncpsid i":"=r"(p)::"memory");return p;}
static void unlock(unsigned p)
{__asm__ volatile("msr primask, %0"::"r"(p):"memory");}
static int valid_mac(const uint8_t *mac)
{unsigned any=0;for(unsigned i=0;i<6;i++)any|=mac[i];return any && !(mac[0]&1);}
static void name_for(char *name,unsigned n)
{
    memset(name,0,16);memcpy(name,"pan",3);
    if(n>=10){name[3]=(char)('0'+n/10);name[4]=(char)('0'+n%10);}
    else name[3]=(char)('0'+n);
}
int pan_eth_start(const struct pan_eth_backend *ops,const uint8_t mac[6],const uint8_t remote[6])
{
    if(!ops || !ops->create || !ops->find || !ops->link || !ops->input || !ops->destroy)return -38;
    if(!mac || !remote || !valid_mac(mac) || !valid_mac(remote))return -22;
    unsigned p=lock();unsigned busy=stats.active || rx_running || tx_running;unlock(p);
    if(busy)return -16;
    if(!bnep_is_connected())return -107;
    /* The network-worker serialization contract protects these native calls. */
    char name[16];unsigned i;
    for(i=0;i<32;i++){name_for(name,i);if(!ops->find(name))break;}
    if(i==32)return -28;
    void *handle=0;
    int result=ops->create(name,mac,PAN_ETH_MTU,pan_eth_tx,&handle);
    if(result<0)return result;
    if(!handle)return -5; /* create contract: failure must not publish a device */
    if(ops->find(name)!=handle){ops->destroy(handle);return -5;}
    p=lock();backend=*ops;native=handle;generation++;
    /* Compressed BNEP RX omits addresses. Both actual peer addresses are
     * mandatory; leaving TinyPAN's reset-time zero addresses corrupts RX. */
    bnep_set_local_addr(mac);bnep_set_remote_addr(remote);
    memset(&stats,0,sizeof(stats));memcpy(stats.name,name,16);
    rx.head=rx.count=tx.head=tx.count=0;stats.active=1;unlock(p);
    /* Publish the TX endpoint before link-up: a native stack may immediately
     * emit ARP/DHCP from link(). A failed link-up rolls that work back. */
    result=ops->link(handle,1);
    if(result<0){pan_eth_stop();return result;}
    return 0;
}
void pan_eth_stop(void)
{
    unsigned p=lock();if(!stats.active){unlock(p);return;}
    stats.active=0;generation++;rx.count=tx.count=0;void *handle=native;native=0;unlock(p);
    backend.link(handle,0);backend.destroy(handle);
}
int pan_eth_tx(const uint8_t *frame,unsigned length)
{
    if(!frame || length<14)return -22;
    if(length>PAN_ETH_FRAME_MAX)return -90;
    unsigned p=lock();
    if(!stats.active){unlock(p);return -100;}
    if(tx.count==PAN_ETH_QUEUE_SIZE){unlock(p);return -11;}
    struct packet *out=&tx.packets[(tx.head+tx.count)%PAN_ETH_QUEUE_SIZE];
    out->length=length;memcpy(out->data,frame,length);tx.count++;unlock(p);return 0;
}
void pan_eth_receive(const bnep_ethernet_frame_t *frame,void *unused)
{
    (void)unused;unsigned p=lock();
    if(!stats.active){unlock(p);return;}
    if(!frame || frame->payload_len>PAN_ETH_MTU || (frame->payload_len && !frame->payload)){
        stats.rx_drops++;stats.last_error=-90;unlock(p);return;
    }
    if(rx.count==PAN_ETH_QUEUE_SIZE){stats.rx_drops++;stats.last_error=-11;unlock(p);return;}
    struct packet *out=&rx.packets[(rx.head+rx.count)%PAN_ETH_QUEUE_SIZE];
    out->length=14+frame->payload_len;memcpy(out->data,frame->dst_addr,6);memcpy(out->data+6,frame->src_addr,6);
    out->data[12]=(uint8_t)(frame->ethertype>>8);out->data[13]=(uint8_t)frame->ethertype;
    if(frame->payload_len)memcpy(out->data+14,frame->payload,frame->payload_len);
    rx.count++;unlock(p);
}
static unsigned peek(struct ring *q,struct packet *out,unsigned *epoch)
{
    unsigned p=lock();if(!stats.active || !q->count){unlock(p);return 0;}
    *epoch=generation;out->length=q->packets[q->head].length;
    memcpy(out->data,q->packets[q->head].data,out->length);unlock(p);return 1;
}
void pan_eth_rx_poll(void)
{
    unsigned p=lock();if(!stats.active || rx_running){unlock(p);return;}rx_running=1;unlock(p);
    unsigned epoch;
    for(unsigned budget=0;budget<PAN_ETH_QUEUE_SIZE && peek(&rx,&rx_work,&epoch);budget++){
        int result=backend.input(native,rx_work.data,rx_work.length);
        p=lock();
        if(epoch!=generation || !stats.active){unlock(p);break;}
        stats.last_error=result;
        if(result==-11){unlock(p);break;}
        rx.head=(rx.head+1)%PAN_ETH_QUEUE_SIZE;rx.count--;
        if(result<0)stats.rx_drops++;else stats.rx_packets++;
        unlock(p);
    }
    p=lock();rx_running=0;unlock(p);
}
void pan_eth_bt_poll(void)
{
    unsigned p=lock();if(!stats.active || tx_running){unlock(p);return;}tx_running=1;unlock(p);
    unsigned epoch;
    if(!bnep_is_connected() || !bnep_drain_control_tx_queue())goto done;
    for(unsigned budget=0;budget<PAN_ETH_QUEUE_SIZE && peek(&tx,&tx_work,&epoch);budget++){
        /* General Ethernet preserves broadcast/multicast, source and EtherType.
         * One complete frame per L2CAP packet; never retry a partial byte range. */
        unsigned type=((unsigned)tx_work.data[12]<<8)|tx_work.data[13];
        int n=bnep_build_general_ethernet(bnep_work,sizeof(bnep_work),tx_work.data,tx_work.data+6,
                                        (uint16_t)type,tx_work.data+14,(uint16_t)(tx_work.length-14));
        int result=n<0?n:hal_bt_l2cap_send(bnep_work,(uint16_t)n);
        p=lock();
        if(epoch!=generation || !stats.active){unlock(p);break;}
        stats.last_error=result;
        if(result){stats.tx_retries++;unlock(p);break;}
        tx.head=(tx.head+1)%PAN_ETH_QUEUE_SIZE;tx.count--;stats.tx_packets++;unlock(p);
    }
done:
    p=lock();tx_running=0;unlock(p);
}
void pan_eth_snapshot(struct pan_eth_stats *out)
{if(out){unsigned p=lock();*out=stats;out->rx_pending=rx.count;out->tx_pending=tx.count;unlock(p);}}
