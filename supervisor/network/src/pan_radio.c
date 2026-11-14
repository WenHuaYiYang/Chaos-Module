/* Link-time forwarding around frozen connection functions. These wrappers
 * change only our own module calls; no kernel operations tables are modified. */
#include "pan_radio.h"
#include "pan_ethernet.h"
#include "bt_probe.h"
#include "bt_addresses.h"
#include <string.h>
#define FN(n,t) ((t)(uintptr_t)(n))
static uint8_t local[6],remote[6];
static unsigned local_ready,remote_ready,pending,epoch;
static int last_error;
static uint32_t retry_at;
static unsigned lock(void){unsigned p;__asm__ volatile("mrs %0, primask\ncpsid i":"=r"(p)::"memory");return p;}
static void unlock(unsigned p){__asm__ volatile("msr primask, %0"::"r"(p):"memory");}
static int valid(const uint8_t *mac){unsigned any=0;for(unsigned i=0;i<6;i++)any|=mac[i];return any && !(mac[0]&1);}
int pan_radio_addresses(uint8_t out_local[6],uint8_t out_remote[6])
{unsigned p=lock();int ready=local_ready && remote_ready;memcpy(out_local,local,6);memcpy(out_remote,remote,6);unlock(p);return ready;}
int pan_radio_error(void){unsigned p=lock();int result=last_error;unlock(p);return result;}
void __real_bt_probe_connect_device(uint32_t);
void __wrap_bt_probe_connect_device(uint32_t index)
{
    struct bt_probe_view view;struct bt_probe_device device;bt_probe_view(&view);
    if((view.phase==0 || view.phase==1) && bt_probe_device(index,&device)){
        unsigned p=lock();for(unsigned i=0;i<6;i++)remote[i]=device.address[5-i];remote_ready=valid(remote);unlock(p);
    }
    __real_bt_probe_connect_device(index);
}
/* "记住的对端直连"是第二条连接入口(v65554 起)。它同样必须记下对端 MAC:
 * DHCP 与 BNEP 的帧头都要用, 缺它 pan_radio_addresses() 恒为 0,
 * pan_ip_poll 的 link 条件永远不成立, 一个包都不发 —— 真机表现就是
 * 连接状态一直停在"正在获取IP"。这里与上面那条入口保持同一口径。 */
int __real_bt_probe_connect_address(const uint8_t address[6],const char *name);
int __wrap_bt_probe_connect_address(const uint8_t address[6],const char *name)
{
    struct bt_probe_view view;bt_probe_view(&view);
    if((view.phase==0 || view.phase==1) && address){
        unsigned p=lock();for(unsigned i=0;i<6;i++)remote[i]=address[5-i];remote_ready=valid(remote);unlock(p);
    }
    return __real_bt_probe_connect_address(address,name);
}
void __real_bnep_init(void);
void __wrap_bnep_init(void)
{
    unsigned p=lock();epoch++;unlock(p);__real_bnep_init();
    uint8_t a[6],b[6];if(pan_radio_addresses(a,b)){bnep_set_local_addr(a);bnep_set_remote_addr(b);}
}
static int cancelled(int valid_arg,int event,void *argument)
{(void)valid_arg;(void)event;(void)argument;unsigned p=lock();pending=0;unlock(p);return 0;}
static int send_work(int valid_arg,int event,void *argument)
{
    (void)valid_arg;(void)event;unsigned p=lock();int current=(unsigned)(uintptr_t)argument==epoch;pending=0;unlock(p);
    uint8_t a[6],b[6];if(current && pan_radio_addresses(a,b)){
        bnep_set_local_addr(a);bnep_set_remote_addr(b);pan_eth_bt_poll();
    }
    return 0;
}
void pan_radio_poll(uint32_t now)
{
    if(!local_ready && (int32_t)(now-retry_at)>=0){
        retry_at=now+1000;
        {
            /* Use the private client created by bt_probe in this task group. */
            uint8_t native[6]={0};int rc=bt_probe_local_address(native);
            uint8_t mac[6];for(unsigned i=0;i<6;i++)mac[i]=native[5-i];
            unsigned p=lock();last_error=rc;
            if(!last_error && valid(mac)){memcpy(local,mac,6);local_ready=1;}
            else if(!last_error)last_error=-22;unlock(p);
        }
    }
    struct pan_eth_stats stats;pan_eth_snapshot(&stats);
    if(!stats.active || !stats.tx_pending)return;
    void *owner=*(void *volatile*)(uintptr_t)BT_OWNER_SLOT;if(!owner)return;
    unsigned p=lock();if(pending){unlock(p);return;}pending=1;unsigned current=epoch;unlock(p);
    FN(BT_QUEUE_EXTERNAL,int(*)(void*,int(*)(int,int,void*),int(*)(int,int,void*),void*,uint8_t))
        (owner,send_work,cancelled,(void*)(uintptr_t)current,0x71);
}
