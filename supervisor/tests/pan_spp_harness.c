#include <assert.h>
#include <stdint.h>
#include <string.h>
#include <stdio.h>
#include "bt_probe.h"
enum {IDLE,SCAN,PAIR,CONNECTING,LINK,STOPPING};
#define MAX_DEVICES 16u
#define BNEP_STATE_WAIT_FOR_CONNECTION_RESPONSE 2
#include STATE
static unsigned ready,queued,commands[256];
static uint32_t lock(void){return 0;}
static void unlock(uint32_t p){(void)p;}
static int prepare(void){return 0;}
static int request(unsigned cmd,const void *p,unsigned n){(void)p;(void)n;++commands[cmd];return 0;}
static void error(int v){state.error=v;}
static void pair_error(unsigned stage,int v){state.error=v;state.error_stage=stage;state.phase=IDLE;}
static void queue(unsigned n){assert(n==1);++queued;}
int chaos_connect_pan_ready(const uint8_t *address){assert(address[0]==1);return ready;}
#include FUNCTIONS
int main(void){
 state.count=1;state.devices[0].address[0]=1;
 bt_probe_connect();assert(state.phase==IDLE&&state.error==-13&&!queued&&!commands[94]);
 ready=1;bt_probe_connect();assert(state.phase==PAIR&&state.connect_ready);
 bt_probe_poll();assert(queued==1&&state.phase==CONNECTING&&!commands[94]&&!commands[0x5f]&&!commands[0x69]);
 state.phase=IDLE;bt_probe_connect();ready=0;bt_probe_poll();
 assert(state.phase==IDLE&&state.error==-13&&queued==1&&!commands[94]);
 puts("PASS PAN only queues L2CAP after live same-peer SPP, skips duplicate bond and rejects SPP loss before handoff");
}
