/* Production stream/pairing state machine with explicit radio/UV seams. */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "../bridge/chaos_connect.h"
static unsigned wanted=1,started=1,now=100,accept_packets=1,received,stops,closes,writes,commands[256],server_calls;
#define CHAOS_CONNECT_RADIO_HOST 1
static unsigned system_gate=1,bond_mode;
static unsigned radio_system_pair_gate(void){return system_gate;}
static void radio_set_system_pair_gate(unsigned value){system_gate=value;}
static int fail_command;static unsigned scan_mode=1;
static void *loop_slot=(void *)0x1234;
#define BT_LOOP ((uintptr_t)&loop_slot)
static uint32_t lock(void){return 0;}
static void unlock(uint32_t p){(void)p;}
static void copy(void *d,const void *s,uint32_t n){memcpy(d,s,n);}
static int chaos_bridge_android_receive(uint32_t generation,const uint8_t *p,unsigned n){
    const uint8_t *f;uint32_t size,id;(void)generation;
    assert(cc_pb_unwrap(p,n,&f,&size,&id)==1);struct cc_frame frame;assert(cc_decode(f,size,&frame)==0);++received;return accept_packets;
}
void chaos_connect_log(const char *s){(void)s;}
uint32_t chaos_connect_now_ms(void){return now;}
static void *client(void){return (void *)0x1111;}
static void *app(void *a,const uint32_t *cb){assert(a==(void *)0x1111 && cb[2]&&cb[3]);return (void *)0x2222;}
static uint32_t gap(void *a,const uint32_t *cb){assert(a==(void *)0x1111 && cb[2]&&cb[5]&&cb[6]&&cb[9]);return 1;}
static int server(void *a,void *h,uint16_t scn,const void *uuid,uint8_t max){
    (void)a;(void)h;const uint32_t *u=uuid;assert(scn==19&&max==1&&u[0]==16&&u[4]==0x43434e31u);++server_calls;return 0;
}
static int stop(void *a,void *h,uint16_t scn){(void)a;(void)h;assert(scn==19);++stops;return 0;}
static int disconnect(void *a,void *h,const uint8_t *peer,uint16_t port){(void)a;(void)h;assert(peer[0]==1&&port>=77);return 0;}
static int ipc(void *a,void *m,uint32_t command){assert(a==(void *)0x1111&&command<256);++commands[command];uint8_t *b=m;
    b[12]=command==(unsigned)fail_command?1:0;if(command==38){b[12]=(uint8_t)scan_mode;memset(b+60,0xcd,8);}
    if(command==92){b[12]=(uint8_t)bond_mode;assert(b[66]==1);}
    if(command==94)assert(b[66]==1);return 0;
}
static void *open_pipe(void *loop,const char *path,void *ready,void *cookie){(void)ready;(void)cookie;assert(loop==loop_slot&&!strcmp(path,"/spp/peer"));return (void *)0x3333;}
static int read_start(void *p,uint16_t n,void *cb,void *cookie){(void)cb;(void)cookie;assert(p==(void *)0x3333&&n==CC_MAX_WIRE);return 0;}
static int write_pipe(void *p,void *b,uint32_t n,void *complete){(void)complete;assert(p==(void *)0x3333&&n>4);uint8_t *q=b;assert((unsigned)(q[0]|q[1]<<8)==n-4);++writes;return 0;}
static void close_pipe(void *p){assert(p==(void *)0x3333);++closes;}
static uintptr_t native(uint32_t a){switch(a){
case 0xc3af1a9:return (uintptr_t)client;case 0xc3b14d1:return (uintptr_t)app;
case 0xc3af3e9:return (uintptr_t)gap;case 0xc3b152d:return (uintptr_t)server;
case 0xc3b1625:return (uintptr_t)disconnect;case 0xc3b1589:return (uintptr_t)stop;case 0xc3ac47d:return (uintptr_t)ipc;
case 0xc3a9cad:return (uintptr_t)open_pipe;case 0xc3a9a75:return (uintptr_t)read_start;
case 0xc3a9bfd:return (uintptr_t)write_pipe;case 0xc3aa051:return (uintptr_t)close_pipe;
default:assert(0);return 0;}}
#define FN(a,t) ((t)native(a))
#include "../bridge/android_radio.inc"
static void step(unsigned enabled){wanted=started=enabled;cc_radio_service(enabled);cc_radio_tick(enabled);}
int main(void){
    step(1);cc_radio_log_owner();assert(server_calls==1&&!radio_pair_until);chaos_connect_pair_start();step(1);
    assert(server_calls==1&&commands[37]==1&&commands[38]==1&&radio_pair_until&&system_gate==0);
    chaos_connect_pair_scan();step(1);assert(commands[30]==1);
    uint8_t device[64]={1,2,3,4,5,6};memcpy(device+12,"Phone",6);radio_gap_device(0,device);
    char name[32];assert(chaos_connect_pair_name(0,name,32)==5&&!strcmp(name,"Phone"));
    chaos_connect_pair_device(0);step(1);assert(commands[0x69]==1&&!commands[94]);
    now+=800;step(1);assert(commands[94]==1);
    bond_mode=1;now+=4000;step(1);assert(commands[94]==1);bond_mode=0;
    radio_gap_pair(0,device);step(1);assert(commands[0x61]==1);
    radio_gap_display(0,device,1,0,123456);step(1);assert(commands[0x62]==1);
    radio_gap_bond(0,device,1,1,0);step(1);assert(!radio_bonded);
    radio_gap_bond(0,device,0,2,0);step(1);assert(!radio_bonded&&radio_pair_until);
    radio_gap_bond(0,device,1,2,1);step(1);assert(!radio_bonded&&radio_pair_until);
    unsigned confirms=commands[0x62];radio_gap_display(0,device,1,3,123456);step(1);assert(commands[0x62]==confirms);
    radio_gap_bond(0,device,1,2,0);step(1);assert(radio_bonded&&!radio_pair_until&&system_gate==1);
    radio_spp_proxy(0,device,0,77,2048,"/spp/peer");step(1);radio_pipe_ready(radio_pipe,0,0);
    assert(radio_ready);uint32_t generation=radio_generation;assert(cc_radio_live((void *)(uintptr_t)generation));
    uint8_t frame[32],packet[256],stream[260];int n=cc_encode(frame,sizeof(frame),0,1,0,1,0,0);n=cc_pb_wrap(packet,sizeof(packet),frame,n,0x12340001);assert(n>0);
    memset(stream,0,4);stream[0]=n;memcpy(stream+4,packet,n);
    for(int i=0;i<n+4;++i)radio_read(radio_pipe,stream+i,1);assert(received==1);
    radio_read(radio_pipe,stream,n+4);radio_read(radio_pipe,stream,n+4);assert(received==3);
    assert(cc_radio_send(generation,packet,n)==0&&radio_write_busy&&writes==1);
    assert(cc_radio_send(generation,packet,n)==-11);radio_written(radio_pipe,radio_output,0);assert(!radio_write_busy);
    assert(cc_radio_send(generation-1,packet,n)==-1);
    accept_packets=0;radio_read(radio_pipe,stream,n+4);step(1);assert(closes==1&&!radio_ready&&radio_generation!=generation);
    radio_spp_proxy(0,device,0,78,2048,"/spp/peer");step(1);radio_pipe_ready(radio_pipe,0,0);
    uint8_t invalid[4]={255,255,255,127};radio_read(radio_pipe,invalid,4);step(1);assert(closes==2&&!radio_ready);
    chaos_connect_pair_start();step(1);now+=60001;step(1);assert(!radio_pair_until);
    for(unsigned mode=0;mode<=2;++mode){scan_mode=mode;chaos_connect_pair_start();step(1);
        assert(!radio_error && radio_pair_until && radio_old_mode[0]==mode && radio_old_mode[1]==1);
        now+=60001;step(1);assert(!radio_pair_until);
    }
    unsigned modes=commands[37];scan_mode=1;chaos_connect_pair_start();step(1);now+=1001;step(1);
    assert(commands[37]>=modes+2&&radio_pair_until);now+=60000;step(1);
    fail_command=37;chaos_connect_pair_start();step(1);
    assert((chaos_connect_radio_status()&0x80000000u) && radio_error_stage==7);
    fail_command=0;step(1);
    wanted=started=0;step(0);assert(stops==1&&!radio_server);
    radio_spp_proxy(0,device,0,79,2048,"/spp/peer");assert(!radio_path_pending);
    puts("Android radio: pairing window/outgoing bond, callback states, UUID, fragmented/coalesced stream, backpressure, generation, shutdown passed");
}
