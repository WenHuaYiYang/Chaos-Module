/* Host-only deterministic transport. Links the production HTTP/WS C files. */
#include "chaos_httpd.h"
#include "chaos_web_platform.h"
#include "pan_ip.h"
#include "pan_ethernet.h"
#include <string.h>
#define RX_MAX 40000u
#define TX_MAX 131072u
struct endpoint {unsigned char rx[RX_MAX],tx[TX_MAX];unsigned size,at,written,open,eof,finished;};
static struct endpoint ends[4];
static unsigned selected,accept_pending,next_id,listening,link_ready,listen_attempts;
static unsigned send_limit,finish_retries,receive_limit;
static int listen_error;
static uint32_t clock_ms;
static unsigned terminal_state,terminal_starts,terminal_stops,terminal_sends,terminal_tx_size,terminal_rx_size,terminal_capacity;
static unsigned sysinfo_flags,reboots,escaped_meminfo;
static char terminal_tx[8192],terminal_rx[8192],password[65];
static struct endpoint *ep(int id){return id>=4&&id<=7?&ends[id-4]:0;}
void test_reset(void){
    chaos_httpd_reset();memset(ends,0,sizeof(ends));selected=accept_pending=listening=0;next_id=4;link_ready=1;clock_ms=1000;
    terminal_state=terminal_starts=terminal_stops=terminal_sends=terminal_tx_size=terminal_rx_size=0;terminal_capacity=8192;
    send_limit=300;receive_limit=RX_MAX;finish_retries=listen_attempts=0;listen_error=0;sysinfo_flags=15;reboots=escaped_meminfo=0;strcpy(password,"host-test-pass");
}
void test_poll(void){chaos_httpd_poll(clock_ms);}
void test_advance(unsigned n){clock_ms+=n;}
void test_set_now(unsigned n){clock_ms=n;}
void test_select(unsigned n){selected=n<4?n:0;}
void test_inject(const char *data,unsigned n){struct endpoint *e=&ends[selected];if(n>RX_MAX)n=RX_MAX;memcpy(e->rx,data,n);e->size=n;e->at=0;}
void test_accept(void){next_id=4+selected;accept_pending=1;ends[selected].written=0;}
void test_set_link(unsigned n){link_ready=n;}
void test_set_send_limit(unsigned n){send_limit=n;}
void test_set_receive_limit(unsigned n){receive_limit=n;}
void test_set_finish_retries(unsigned n){finish_retries=n;}
void test_set_listen_error(int n){listen_error=n;}
void test_set_sysinfo_flags(unsigned n){sysinfo_flags=n;}
void test_set_escaped_meminfo(unsigned n){escaped_meminfo=n;}
void test_peer_eof(void){ends[selected].eof=1;}
void test_set_password(const char *s){unsigned n=(unsigned)strlen(s);if(n>=sizeof(password))n=sizeof(password)-1;memcpy(password,s,n);password[n]=0;}
unsigned test_listening(void){return listening;}
unsigned test_listen_attempts(void){return listen_attempts;}
unsigned test_open(void){return ends[selected].open;}
unsigned test_finished(void){return ends[selected].finished;}
unsigned test_output_size(void){return ends[selected].written;}
const char *test_output(void){return (const char*)ends[selected].tx;}
void test_clear_output(void){ends[selected].written=0;}
unsigned test_terminal_starts(void){return terminal_starts;}
unsigned test_terminal_stops(void){return terminal_stops;}
unsigned test_terminal_sends(void){return terminal_sends;}
unsigned test_terminal_tx_size(void){return terminal_tx_size;}
const char *test_terminal_tx(void){return terminal_tx;}
void test_set_terminal_state(unsigned n){terminal_state=n;}
void test_set_terminal_capacity(unsigned n){terminal_capacity=n;}
void test_terminal_reap(void){terminal_state=3;terminal_rx_size=terminal_tx_size=0;}
unsigned test_reboots(void){return reboots;}
int chaos_web_credentials(char *u,unsigned uc,char *p,unsigned pc){if(!password[0])return -13;if(uc<6||pc<=strlen(password))return -22;strcpy(u,"chaos");strcpy(p,password);return 0;}
int chaos_web_terminal_start(void){if(terminal_state==5)return -16;if(terminal_state==1||terminal_state==2)return 0;terminal_starts++;terminal_state=2;terminal_rx_size=5;memcpy(terminal_rx,"nsh> ",5);return 0;}
void chaos_web_terminal_stop(void){if(terminal_state==1||terminal_state==2){terminal_stops++;terminal_state=5;}}
int chaos_web_terminal_state(void){return (int)terminal_state;}
int chaos_web_terminal_error(void){return 0;}
int chaos_web_terminal_send(const char *s,unsigned n){terminal_sends++;if(terminal_state!=2)return -107;if(n+terminal_tx_size>terminal_capacity||n+terminal_tx_size>sizeof(terminal_tx))return -11;memcpy(terminal_tx+terminal_tx_size,s,n);terminal_tx_size+=n;return 0;}
unsigned chaos_web_terminal_receive(char *out,unsigned cap){unsigned n=terminal_rx_size;if(n>cap)n=cap;memcpy(out,terminal_rx,n);memmove(terminal_rx,terminal_rx+n,terminal_rx_size-n);terminal_rx_size-=n;return n;}
int chaos_web_reboot(void){reboots++;return 0;}
int pan_ip_ready(void){return (int)link_ready;}
uint32_t pan_ip_now(void){return clock_ms;}
void pan_ip_info(struct pan_ip_info *out){memset(out,0,sizeof(*out));out->ip[0]=192;out->ip[1]=168;out->ip[2]=44;out->ip[3]=2;out->mask[0]=out->mask[1]=out->mask[2]=255;out->gateway[0]=192;out->gateway[1]=168;out->gateway[2]=44;out->gateway[3]=1;out->dns[0]=10;out->dns[1]=23;out->dns[2]=45;out->dns[3]=67;out->mtu=1500;out->ready=link_ready;}
int pan_tcp_listen(uint16_t port){listen_attempts++;if(listen_error)return listen_error;if(port!=80)return -22;if(listening)return -98;listening=1;return 1;}
void pan_tcp_unlisten(int id){(void)id;listening=0;}
int pan_tcp_accept(int id){if(id!=1||!accept_pending)return -11;accept_pending=0;struct endpoint *e=ep((int)next_id);e->open=1;e->finished=e->eof=0;return (int)next_id;}
int pan_tcp_state(int id){struct endpoint *e=ep(id);if(!e||!e->open)return -9;return e->eof?PAN_TCP_EOF:PAN_TCP_OPEN;}
int pan_tcp_receive(int id,void *out,unsigned cap){struct endpoint *e=ep(id);if(!e||!e->open)return -9;unsigned n=e->size-e->at;if(!n)return -11;if(n>cap)n=cap;if(n>receive_limit)n=receive_limit;memcpy(out,e->rx+e->at,n);e->at+=n;return (int)n;}
int pan_tcp_send(int id,const void *data,unsigned n){struct endpoint *e=ep(id);if(!e||!e->open)return -9;if(n>send_limit)n=send_limit;if(!n||e->written+n>TX_MAX)return -11;memcpy(e->tx+e->written,data,n);e->written+=n;return (int)n;}
int pan_tcp_finish(int id){struct endpoint *e=ep(id);if(!e)return -9;if(finish_retries){finish_retries--;return -11;}e->open=0;e->finished=1;return 0;}
void pan_tcp_close(int id){struct endpoint *e=ep(id);if(e)e->open=0;}
void pan_eth_snapshot(struct pan_eth_stats *out){memset(out,0,sizeof(*out));out->active=1;out->rx_packets=1234;out->tx_packets=567;strcpy(out->name,"pan0");}
void chaos_web_sysinfo_read(struct chaos_web_sysinfo *out){memset(out,0,sizeof(*out));strcpy(out->model,"Xiaomi Band 10 Pro");strcpy(out->firmware,"3.101.043");strcpy(out->build,"host fixture, not firmware measurement");strcpy(out->meminfo,"Uptime: 3725.00 sec\nFree: 800000\n");if(escaped_meminfo)memset(out->meminfo,1,sizeof(out->meminfo)-1);out->valid=sysinfo_flags;out->uptime_seconds=3725;out->heap_total=1000000;out->heap_free=800000;out->heap_largest=600000;}
