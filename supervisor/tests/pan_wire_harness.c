/* Test-only virtual NAP wire and Bluetooth-control doubles. Not linked in KO. */
#include "pan_ip.h"
#include "pan_ethernet.h"
#include "network_service.h"
#include <string.h>
/* The production Rust link supplies this AAPCS memory intrinsic. */
void __aeabi_memclr4(void *out,unsigned n){memset(out,0,n);}
static const uint8_t local[6]={2,0,0,0,0,1},remote[6]={2,0,0,0,0,2};
static uint8_t frames[64][1530],incoming[1530];
static unsigned lengths[64],head,count;
uint8_t test_wire[1530],test_read_buffer[8192];
static struct bt_probe_view mock_bt;
/* 生产代码里 pan_radio 由 --wrap 提供对端 MAC; 这里只回一个"地址就绪"布尔,
 * 用来区分 -110(手机没给 IP) 与 -97(地址没就绪, 一个包都没发)。 */
unsigned test_radio_ready=1;
int pan_radio_addresses(uint8_t local[6],uint8_t remote[6])
{(void)local;(void)remote;return (int)test_radio_ready;}
unsigned test_scans,test_disconnects,test_confirms,test_connects,test_connected_index;
unsigned test_address_connects;uint8_t test_connected_address[6];
unsigned test_sdp_starts,test_sdp_cancels,test_sdp_pending,test_sdp_hold;
struct network_view test_view;
int hal_bt_l2cap_send(const uint8_t *data,uint16_t length)
{if(count==64 || length>1530)return -11;unsigned i=(head+count)%64;memcpy(frames[i],data,length);lengths[i]=length;count++;return 0;}
void hal_bt_l2cap_request_can_send_now(void){}
void test_boot(void)
{
    bnep_init();bnep_set_local_addr(local);bnep_set_remote_addr(remote);
    bnep_register_frame_callback(pan_eth_receive,0);bnep_on_l2cap_connected();
    static const uint8_t response[4]={1,2,0,0};bnep_handle_incoming(response,4);head=count=0;
}
void test_step(unsigned now){pan_ip_poll(1,local,remote,now);pan_eth_bt_poll();}
void test_down(unsigned now){pan_ip_poll(0,local,remote,now);}
unsigned test_pop(void)
{if(!count)return 0;unsigned n=lengths[head];memcpy(test_wire,frames[head],n);head=(head+1)%64;count--;return n;}
void test_input(unsigned length)
{
    if(length<14 || length>1514)return;
    int n=bnep_build_general_ethernet(incoming,sizeof(incoming),test_wire,test_wire+6,
        (uint16_t)(test_wire[12]<<8|test_wire[13]),test_wire+14,(uint16_t)(length-14));
    if(n>0)bnep_handle_incoming(incoming,(uint16_t)n);
}
void test_ip_status(void){pan_ip_status((char*)test_read_buffer,sizeof(test_read_buffer));}
void bt_probe_view(struct bt_probe_view *out){*out=mock_bt;}
void bt_probe_scan(void){test_scans++;mock_bt.phase=1;mock_bt.error=0;}
void bt_probe_disconnect(void){test_disconnects++;mock_bt.phase=mock_bt.cid=mock_bt.pair_confirm=0;}
void bt_probe_connect_device(uint32_t index){test_connects++;test_connected_index=index;mock_bt.phase=2;}
void bt_probe_connect(void){test_confirms++;mock_bt.pair_confirm=0;}
int bt_probe_connect_address(const uint8_t address[6],const char *name)
{(void)name;test_address_connects++;memcpy(test_connected_address,address,6);mock_bt.count=1;mock_bt.phase=2;return 0;}
void bt_probe_bnep(void){mock_bt.bnep=3;}
int bt_probe_device(uint32_t index,struct bt_probe_device *out)
{if(index>=mock_bt.count)return 0;memset(out,0,sizeof(*out));out->address[0]=(uint8_t)index;out->name[0]='A'+index;return 1;}
void test_scan_done(unsigned n){mock_bt.phase=0;mock_bt.count=n;}
void test_pairing(unsigned passkey){mock_bt.phase=2;mock_bt.pair_confirm=1;mock_bt.passkey=passkey;}
void test_link_error(int error){mock_bt.error=error;}
void test_linked(unsigned cid){mock_bt.phase=4;mock_bt.bnep=3;mock_bt.cid=(uint16_t)cid;}
int network_sdp_start(const uint8_t address[6]){(void)address;test_sdp_starts++;test_sdp_pending=1;return 0;}
void network_sdp_poll(void){}
int network_sdp_result(void){if(test_sdp_hold)return -11;test_sdp_pending=0;return 1;}
void network_sdp_cancel(void){test_sdp_cancels++;test_sdp_pending=0;}
void test_network_poll(void){network_service_poll();network_service_view(&test_view);}
/* Include the unmodified production state machine in this test translation
 * unit, allowing an otherwise unreachable full ring to be exercised too. */
#include "../network/src/network_service.c"
void test_fill_command_queue(void){for(unsigned i=0;i<COMMAND_QUEUE;i++)request(100+i,0);}
unsigned test_command_queue_count(void){return command_count;}
/* 记住的对端直连入口：地址就是调用方给的这 6 个字节，不需要先扫描。 */
int test_connect_saved(void)
{static const uint8_t a[6]={0x11,0x22,0x33,0x44,0x55,0x66};return network_connect_address(a,"phone");}
