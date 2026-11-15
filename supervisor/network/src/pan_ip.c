/* Module-owned lightweight IPv4/TCP stack. Single PAN-worker ownership.
 * Ethernet queues/TinyPAN are the only network device, not POSIX sockets. */
#include "pan_ip.h"
#include "pan_ethernet.h"
/* 043: 无 q66 terminal_native 依赖 */
#include "lwip/init.h"
#include "lwip/netif.h"
#include "lwip/etharp.h"
#include "lwip/dhcp.h"
#include "lwip/dns.h"
#include "lwip/tcp.h"
#include "lwip/udp.h"
#include "lwip/pbuf.h"
#include "lwip/timeouts.h"
#include "netif/ethernet.h"
#include <string.h>
#define CONNECTIONS 6u
#define RECEIVE_SIZE 4096u
#define LISTENERS 4u
struct connection {
    struct tcp_pcb *pcb;
    unsigned used,id,state,head,count;
    uint16_t port;
    int error;
    uint32_t deadline;
    uint8_t receive[RECEIVE_SIZE];
};
/* More than one listen port is required by the resident services: SSHD (22),
 * the built-in HTTP server (80) and later protocol servers each own a slot.
 * Every callback still runs on the single PAN worker. */
struct listener {
    struct tcp_pcb *pcb;
    unsigned used,port;
    int pending; /* connection id accepted on this listener, 0 if none */
};
static struct connection connections[CONNECTIONS];
static struct listener listeners[LISTENERS];
static struct udp_pcb *datagrams;
static struct netif interface;
static unsigned initialized,created,active,sequence;
static uint32_t now_ms,random_state=0x53595041,last_ip;
static uint32_t dhcp_dns,dns_fallback;
static unsigned faulted;
static int link_error;
static char interface_name[16],published[96];
static struct pan_ip_info published_info;
static unsigned lock(void){unsigned p;__asm__ volatile("mrs %0, primask\ncpsid i":"=r"(p)::"memory");return p;}
static void unlock(unsigned p){__asm__ volatile("msr primask, %0"::"r"(p):"memory");}
uint32_t sys_now(void){return now_ms;}
uint32_t pan_ip_now(void){return now_ms;}
uint32_t pan_lwip_rand(void)
{random_state^=random_state<<13;random_state^=random_state>>17;random_state^=random_state<<5;return random_state;}
void pan_lwip_assert(const char *message)
{
    (void)message;__atomic_store_n(&faulted,1,__ATOMIC_RELEASE);
    /* Stop only this worker on an internal invariant violation. Do not run
     * onward through invalid protocol state or reset the entire device. */
    /* WFI only waits for an IRQ; it does not make this RTOS task unready.
     * Use the .043 usleep ABI already audited by the NSH backend so this
     * worker cannot starve lower-priority system tasks after an assertion. */
    for(;;)((int(*)(uint32_t))(uintptr_t)0x0c1f5d0du)(1000000u);
}
static int native_error(err_t e)
{
    switch(e){case ERR_MEM:case ERR_BUF:return 11;case ERR_TIMEOUT:return 110;
    case ERR_RTE:return 101;case ERR_ABRT:return 103;case ERR_RST:return 104;
    case ERR_CLSD:return 107;case ERR_CONN:return 107;default:return 5;}
}
static struct connection *find(int id)
{if(id<=0)return 0;for(unsigned i=0;i<CONNECTIONS;i++)if(connections[i].used && connections[i].id==(unsigned)id)return &connections[i];return 0;}
static unsigned connection_id(const struct connection *c)
{
    /* Six slots require a stride larger than the slot index. The old
     * sequence*4+slot scheme aliased a live handle every second generation. */
    sequence=(sequence+1)&0x01ffffffu;if(!sequence)sequence=1;
    return sequence*8u+(unsigned)(c-connections)+1u;
}
static struct listener *listener_find(int id)
{if(id<=0)return 0;for(unsigned i=0;i<LISTENERS;i++)if(listeners[i].used && (int)(i+1u)==id)return &listeners[i];return 0;}
static int listener_id_of(const struct listener *l){return (int)(l-listeners)+1;}
/* A closed connection must not stay queued as a pending accept on any port. */
static void clear_pending(int id)
{for(unsigned i=0;i<LISTENERS;i++)if(listeners[i].pending==id)listeners[i].pending=0;}
static void dispose(struct connection *c)
{
    if(c->pcb){tcp_arg(c->pcb,0);tcp_err(c->pcb,0);tcp_recv(c->pcb,0);tcp_abort(c->pcb);c->pcb=0;}
}
static void connection_error(void *argument,err_t error)
{struct connection *c=argument;if(c){c->pcb=0;c->error=native_error(error);c->state=PAN_TCP_ERROR;}}
static err_t received(void *argument,struct tcp_pcb *pcb,struct pbuf *buffer,err_t error)
{
    struct connection *c=argument;
    if(!c){if(buffer)pbuf_free(buffer);return ERR_OK;}
    if(error!=ERR_OK)return error;
    if(!buffer){
        c->state=PAN_TCP_EOF;tcp_arg(pcb,0);tcp_err(pcb,0);tcp_recv(pcb,0);
        if(tcp_close(pcb)!=ERR_OK){tcp_abort(pcb);c->pcb=0;return ERR_ABRT;}
        c->pcb=0;return ERR_OK;
    }
    if(buffer->tot_len>RECEIVE_SIZE-c->count)return ERR_MEM;
    unsigned tail=(c->head+c->count)%RECEIVE_SIZE,n=buffer->tot_len;
    unsigned first=RECEIVE_SIZE-tail;if(first>n)first=n;
    pbuf_copy_partial(buffer,c->receive+tail,(u16_t)first,0);
    if(first<n)pbuf_copy_partial(buffer,c->receive,(u16_t)(n-first),(u16_t)first);
    c->count+=n;pbuf_free(buffer);return ERR_OK;
}
static err_t connected(void *argument,struct tcp_pcb *pcb,err_t error)
{
    (void)pcb;struct connection *c=argument;
    if(error){c->error=native_error(error);c->state=PAN_TCP_ERROR;}
    else c->state=PAN_TCP_OPEN;return ERR_OK;
}
static void begin_connect(struct connection *c,const ip_addr_t *address)
{
    c->pcb=tcp_new_ip_type(IPADDR_TYPE_V4);
    if(!c->pcb){c->state=PAN_TCP_ERROR;c->error=12;return;}
    tcp_arg(c->pcb,c);tcp_err(c->pcb,connection_error);tcp_recv(c->pcb,received);
    c->state=PAN_TCP_CONNECTING;
    err_t e=tcp_connect(c->pcb,address,c->port,connected);
    if(e){dispose(c);c->state=PAN_TCP_ERROR;c->error=native_error(e);}
}
static void resolved(const char *name,const ip_addr_t *address,void *argument)
{
    (void)name;struct connection *c=find((int)(uintptr_t)argument);
    if(!c || c->state!=PAN_TCP_RESOLVING)return;
    if(!address){c->state=PAN_TCP_ERROR;c->error=113;return;}
    begin_connect(c,address);
}
int pan_ip_ready(void)
{return active && !__atomic_load_n(&faulted,__ATOMIC_ACQUIRE) && !ip4_addr_isany_val(*netif_ip4_addr(&interface));}
int pan_ip_faulted(void){return (int)__atomic_load_n(&faulted,__ATOMIC_ACQUIRE);}
int pan_tcp_open(const char *host,uint16_t port)
{
    if(!host || !*host || strlen(host)>253 || !port)return -22;
    if(!pan_ip_ready())return -100;
    struct connection *c=0;for(unsigned i=0;i<CONNECTIONS;i++)if(!connections[i].used){c=&connections[i];break;}
    if(!c)return -24;
    memset(c,0,sizeof(*c));c->used=1;c->id=connection_id(c);c->port=port;c->state=PAN_TCP_RESOLVING;c->deadline=now_ms+30000;
    ip_addr_t address;err_t e=dns_gethostbyname(host,&address,resolved,(void*)(uintptr_t)c->id);
    if(e==ERR_OK)begin_connect(c,&address);
    else if(e!=ERR_INPROGRESS){c->state=PAN_TCP_ERROR;c->error=native_error(e);}
    return (int)c->id;
}
/* Numeric-literal connect used by the LAN bridge's relay target. Avoids
 * depending on dns_gethostbyname accepting dotted-quad strings. */
int pan_tcp_open_ip(const uint8_t address[4],uint16_t port)
{
    struct connection *c=0;
    ip_addr_t target;
    if(!address || !port)return -22;
    if(!pan_ip_ready())return -100;
    for(unsigned i=0;i<CONNECTIONS;i++)if(!connections[i].used){c=&connections[i];break;}
    if(!c)return -24;
    memset(c,0,sizeof(*c));c->used=1;c->id=connection_id(c);c->port=port;c->state=PAN_TCP_CONNECTING;c->deadline=now_ms+30000;
    IP_ADDR4(&target,address[0],address[1],address[2],address[3]);
    begin_connect(c,&target);
    return (int)c->id;
}
int pan_udp_send(const uint8_t address[4],uint16_t port,const void *data,unsigned length)
{
    struct pbuf *packet;
    ip_addr_t target;
    err_t e;
    if(!address || !port || (!data && length) || length>1400u)return -22;
    if(!pan_ip_ready())return -100;
    if(!datagrams){datagrams=udp_new_ip_type(IPADDR_TYPE_V4);if(!datagrams)return -12;}
    packet=pbuf_alloc(PBUF_TRANSPORT,(u16_t)length,PBUF_RAM);
    if(!packet)return -12;
    if(length && pbuf_take(packet,data,(u16_t)length)!=ERR_OK){pbuf_free(packet);return -12;}
    IP_ADDR4(&target,address[0],address[1],address[2],address[3]);
    e=udp_sendto(datagrams,packet,&target,port);
    pbuf_free(packet);
    return e==ERR_OK?0:-native_error(e);
}
int pan_tcp_state(int id){struct connection *c=find(id);return c?(int)c->state:PAN_TCP_ERROR;}
int pan_tcp_error(int id){struct connection *c=find(id);return c?c->error:9;}
int pan_tcp_send(int id,const void *data,unsigned length)
{
    struct connection *c=find(id);if(!c)return -9;
    if(c->state==PAN_TCP_ERROR)return -c->error;
    if(c->state!=PAN_TCP_OPEN || !c->pcb)return -11;
    if(!data && length)return -22;
    unsigned n=length;if(n>tcp_sndbuf(c->pcb))n=tcp_sndbuf(c->pcb);if(n>1460)n=1460;
    if(!n)return length?-11:0;
    err_t e=tcp_write(c->pcb,data,(u16_t)n,TCP_WRITE_FLAG_COPY);
    if(e)return -native_error(e);
    (void)tcp_output(c->pcb);return (int)n;
}
int pan_tcp_receive(int id,void *data,unsigned length)
{
    struct connection *c=find(id);if(!c)return -9;if(!data && length)return -22;
    unsigned n=length;if(n>c->count)n=c->count;
    for(unsigned i=0;i<n;i++){((uint8_t*)data)[i]=c->receive[c->head];c->head=(c->head+1)%RECEIVE_SIZE;}
    c->count-=n;if(n){if(c->pcb)tcp_recved(c->pcb,(u16_t)n);return (int)n;}
    if(c->state==PAN_TCP_ERROR)return -c->error;
    return c->state==PAN_TCP_EOF?0:-11;
}
void pan_tcp_close(int id){struct connection *c=find(id);if(c){dispose(c);c->used=0;c->count=0;clear_pending(id);}}
int pan_tcp_finish(int id)
{
    struct connection *c=find(id);if(!c)return -9;if(!c->pcb){pan_tcp_close(id);return 0;}
    struct tcp_pcb *pcb=c->pcb;tcp_arg(pcb,0);tcp_err(pcb,0);tcp_recv(pcb,0);
    err_t e=tcp_close(pcb);if(e){tcp_arg(pcb,c);tcp_err(pcb,connection_error);tcp_recv(pcb,received);return -native_error(e);}
    c->pcb=0;c->used=0;c->count=0;clear_pending(id);return 0;
}
int pan_tcp_shutdown_write(int id)
{
    struct connection *c=find(id);if(!c || !c->pcb)return -9;
    err_t e=tcp_shutdown(c->pcb,0,1);return e?-native_error(e):0;
}
static err_t accept_connection(void *argument,struct tcp_pcb *pcb,err_t error)
{
    struct listener *listener=argument;
    struct connection *c=0;
    if(error || !listener || !listener->used || listener->pending){if(pcb)tcp_abort(pcb);return ERR_ABRT;}
    for(unsigned i=0;i<CONNECTIONS;i++)if(!connections[i].used){c=&connections[i];break;}
    if(!c){tcp_abort(pcb);return ERR_ABRT;}
    memset(c,0,sizeof(*c));c->used=1;c->id=connection_id(c);c->pcb=pcb;c->state=PAN_TCP_OPEN;
    tcp_arg(pcb,c);tcp_err(pcb,connection_error);tcp_recv(pcb,received);listener->pending=(int)c->id;return ERR_OK;
}
int pan_tcp_listen(uint16_t port)
{
    struct listener *listener=0;
    struct tcp_pcb *pcb;
    if(!port)return -22;
    if(!pan_ip_ready())return -100;
    for(unsigned i=0;i<LISTENERS;i++)if(listeners[i].used && listeners[i].port==port)return -98;
    for(unsigned i=0;i<LISTENERS;i++)if(!listeners[i].used){listener=&listeners[i];break;}
    if(!listener)return -98;
    pcb=tcp_new_ip_type(IPADDR_TYPE_V4);if(!pcb)return -12;
    if(tcp_bind(pcb,IP_ANY_TYPE,port)!=ERR_OK){tcp_abort(pcb);return -98;}
    { struct tcp_pcb *listening=tcp_listen_with_backlog(pcb,2);
      if(!listening){tcp_abort(pcb);return -12;}
      pcb=listening;
    }
    memset(listener,0,sizeof(*listener));
    listener->pcb=pcb;listener->used=1;listener->port=port;
    tcp_arg(pcb,listener);tcp_accept(pcb,accept_connection);
    return listener_id_of(listener);
}
int pan_tcp_accept(int id)
{struct listener *l=listener_find(id);int n;if(!l)return -9;if(!l->pending)return -11;n=l->pending;l->pending=0;return n;}
void pan_tcp_unlisten(int id)
{
    struct listener *l=listener_find(id);
    if(!l)return;
    if(l->pending){int pending=l->pending;l->pending=0;pan_tcp_close(pending);}
    if(l->pcb){tcp_close(l->pcb);l->pcb=0;}
    l->used=0;l->port=0;
}
int pan_tcp_peer(int id,uint8_t address[4],uint16_t *port)
{struct connection *c=find(id);if(!c || !c->pcb || !address || !port)return -9;memcpy(address,&ip_2_ip4(&c->pcb->remote_ip)->addr,4);*port=c->pcb->remote_port;return 0;}
static err_t output(struct netif *netif,struct pbuf *packet)
{
    (void)netif;static uint8_t flat[1514];
    if(packet->tot_len>sizeof(flat))return ERR_BUF;
    pbuf_copy_partial(packet,flat,packet->tot_len,0);
    return pan_eth_tx(flat,packet->tot_len)==0?ERR_OK:ERR_MEM;
}
static err_t initialize_interface(struct netif *netif)
{
    netif->name[0]='p';netif->name[1]='a';netif->hostname="SysPatch";
    netif->output=etharp_output;netif->linkoutput=output;netif->mtu=1500;netif->hwaddr_len=6;
    netif->flags=NETIF_FLAG_BROADCAST|NETIF_FLAG_ETHARP|NETIF_FLAG_ETHERNET;return ERR_OK;
}
static int create(const char *name,const uint8_t *mac,unsigned mtu,int(*tx)(const uint8_t*,unsigned),void **handle)
{
    (void)tx;if(created || mtu!=1500)return -22;
    memset(&interface,0,sizeof(interface));
    if(!netif_add(&interface,0,0,0,0,initialize_interface,ethernet_input))return -12;
    memcpy(interface.hwaddr,mac,6);memcpy(interface_name,name,16);created=1;*handle=&interface;return 0;
}
static void *lookup(const char *name){return created && !strcmp(name,interface_name)?&interface:0;}
static int link(void *handle,int up)
{
    if(handle!=&interface || !created)return -19;
    if(up){netif_set_default(&interface);netif_set_link_up(&interface);netif_set_up(&interface);return dhcp_start(&interface)==ERR_OK?0:-12;}
    dhcp_stop(&interface);netif_set_down(&interface);netif_set_link_down(&interface);return 0;
}
static int input(void *handle,const uint8_t *frame,unsigned length)
{
    if(handle!=&interface || !created)return -19;
    struct pbuf *packet=pbuf_alloc(PBUF_RAW,(u16_t)length,PBUF_POOL);if(!packet)return -11;
    if(pbuf_take(packet,frame,(u16_t)length)!=ERR_OK){pbuf_free(packet);return -5;}
    err_t e=interface.input(packet,&interface);if(e)pbuf_free(packet);return e?-5:0;
}
static void destroy(void *handle)
{if(handle==&interface && created){dhcp_cleanup(&interface);netif_remove(&interface);created=0;interface_name[0]=0;}}
static const struct pan_eth_backend backend={create,lookup,link,input,destroy};
static void number(char *out,unsigned *p,unsigned n)
{char b[12];unsigned k=0;do{b[k++]=(char)('0'+n%10);n/=10;}while(n);while(k && *p<94)out[(*p)++]=b[--k];}
static void append(char *out,unsigned *p,const char *s){while(*s && *p<94)out[(*p)++]=*s++;}
static void publish(void)
{
    char next[96]={0};unsigned p=0;
    struct pan_ip_info info={0};
    if(active){memcpy(info.ip,&netif_ip4_addr(&interface)->addr,4);memcpy(info.mask,&netif_ip4_netmask(&interface)->addr,4);
        memcpy(info.gateway,&netif_ip4_gw(&interface)->addr,4);memcpy(info.dns,&dhcp_dns,4);
        memcpy(info.mac,interface.hwaddr,6);info.mtu=interface.mtu;info.ready=(unsigned)pan_ip_ready();}
    if(!active){append(next,&p,"等待 PAN");if(link_error){append(next,&p," / 错误 ");number(next,&p,(unsigned)-link_error);}}
    else if(!pan_ip_ready())append(next,&p,"DHCP 获取 IP…");
    else {append(next,&p,"IP ");const uint8_t *ip=(const uint8_t*)&netif_ip4_addr(&interface)->addr;
        for(unsigned i=0;i<4;i++){if(i)next[p++]='.';number(next,&p,ip[i]);}}
    unsigned mask=lock();memcpy(published,next,sizeof(published));published_info=info;unlock(mask);
}
void pan_ip_info(struct pan_ip_info *out)
{if(out){unsigned mask=lock();*out=published_info;unlock(mask);
    if(__atomic_load_n(&faulted,__ATOMIC_ACQUIRE))out->ready=0;}}
void pan_ip_status(char *out,unsigned capacity)
{
    if(!out || !capacity)return;unsigned n=capacity<sizeof(published)?capacity:sizeof(published);
    unsigned mask=lock();memcpy(out,published,n);unlock(mask);out[n-1]=0;
    if(__atomic_load_n(&faulted,__ATOMIC_ACQUIRE)){const char *s="协议栈断言，已停用";unsigned i=0;while(s[i] && i+1<capacity){out[i]=s[i];i++;}out[i]=0;}
}
void pan_ip_poll(int connected_link,const uint8_t local[6],const uint8_t remote[6],uint32_t now)
{
    now_ms=now;if(!initialized){random_state^=now;lwip_init();initialized=1;}
    if(active && !connected_link){
        for(unsigned i=0;i<LISTENERS;i++)if(listeners[i].used)pan_tcp_unlisten((int)i+1);
        for(unsigned i=0;i<CONNECTIONS;i++)if(connections[i].used)pan_tcp_close((int)connections[i].id);
        pan_eth_stop();active=0;last_ip=0;dhcp_dns=dns_fallback=0;dns_setserver(0,0);dns_setserver(1,0);
    }
    if(connected_link && !active){link_error=pan_eth_start(&backend,local,remote);if(!link_error)active=1;}
    if(active)pan_eth_rx_poll();
    sys_check_timeouts();
    if(active){
        uint32_t ip=netif_ip4_addr(&interface)->addr;
        if(last_ip && ip!=last_ip)for(unsigned i=0;i<CONNECTIONS;i++)if(connections[i].used)pan_tcp_close((int)connections[i].id);
        uint32_t dns_now=ip_2_ip4(dns_getserver(0))->addr;
        if(ip && (ip!=last_ip || dns_now!=dns_fallback))dhcp_dns=dns_now;
        last_ip=ip;
        if(ip && ip_addr_isany(dns_getserver(0)) && !ip4_addr_isany_val(*netif_ip4_gw(&interface))){
            ip_addr_t dns;ip_addr_copy_from_ip4(dns,*netif_ip4_gw(&interface));dns_setserver(0,&dns);dns_fallback=ip_2_ip4(&dns)->addr;}
    }
    for(unsigned i=0;i<CONNECTIONS;i++){struct connection *c=&connections[i];
        if(c->used && (c->state==PAN_TCP_RESOLVING || c->state==PAN_TCP_CONNECTING) && (int32_t)(now_ms-c->deadline)>=0){dispose(c);c->state=PAN_TCP_ERROR;c->error=110;}}
    publish();
}
