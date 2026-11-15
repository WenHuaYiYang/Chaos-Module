#ifndef CHAOS_BUILD_ID
#define CHAOS_BUILD_ID "chaos-dev"
#endif

/* Chaos p67 3.101.043 C/Rust 胶水层。Rust 页面只调这里的扁平 API：
 * 命令入队后由 PAN worker 执行（network_service 的单写者约定），状态用文本快照
 * 回传，避免在 FFI 上展开结构体。所有地址/协议事实见 Ghidra/bt-ipc-043.md。 */
#include "network_service.h"
#include "bt_probe.h"
#include "pan_ip.h"
#include "../web/chaos_httpd.h"
#include "../web/chaos_web_platform.h"
#include <stdint.h>
#include <string.h>

#ifdef CHAOS_NET_GLUE_TEST
static unsigned lock(void){return 0;}
static void unlock(unsigned p){(void)p;}
#else
static unsigned lock(void){unsigned p;__asm__ volatile("mrs %0, primask\ncpsid i":"=r"(p)::"memory");return p;}
static void unlock(unsigned p){__asm__ volatile("msr primask, %0"::"r"(p):"memory");}
#endif
static char web_user[32];static char web_pass[65];static unsigned have_credential;

void chaos_pan_enable(int on){network_enable(on);}
void chaos_pan_scan(void){network_scan();}
void chaos_pan_connect(unsigned index){network_connect(index);}
int chaos_pan_connect_address(const unsigned char *address,const char *name){return network_connect_address(address,name);}
void chaos_pan_disconnect(void){network_disconnect();}
void chaos_pan_pair_confirm(void){network_pair_confirm();}
void chaos_httpd_set_enable(int on){unsigned p=lock();chaos_httpd_enable(on && have_credential);unlock(p);}
void chaos_network_view(struct network_view *out){network_service_view(out);}

static void append(char *out,unsigned *pos,unsigned cap,const char *s)
{while(*s && *pos+1<cap)out[(*pos)++]=*s++;}
static void append_num(char *out,unsigned *pos,unsigned cap,unsigned v)
{char d[12];unsigned n=0;do{d[n++]=(char)('0'+v%10);v/=10;}while(v);while(n && *pos+1<cap)out[(*pos)++]=d[--n];}
static void append_ip(char *out,unsigned *pos,unsigned cap,const uint8_t *ip)
{for(unsigned i=0;i<4;i++){if(i)append(out,pos,cap,".");append_num(out,pos,cap,ip[i]);}}
static void append_mac(char *out,unsigned *pos,unsigned cap,const uint8_t *mac)
{static const char h[]="0123456789ABCDEF";
 for(unsigned i=0;i<6;i++){if(i)append(out,pos,cap,":");if(*pos+2>=cap)break;out[(*pos)++]=h[mac[5-i]>>4];out[(*pos)++]=h[mac[5-i]&15];out[*pos]=0;}}

static const char *const PHASES[]={"空闲","扫描中","过滤NAP","连接中","等待配对","协商BNEP","获取IP","已就绪"};

/* 页面展示用文本快照：状态行 + 设备列表(前8) + IP 信息。 */
void chaos_pan_snapshot(char *out, unsigned cap)
{
    if(!out||!cap)return;out[0]=0;unsigned pos=0;
    struct network_view v;network_service_view(&v);
    struct pan_ip_info ip;pan_ip_info(&ip);
    if(!v.enabled){append(out,&pos,cap,"未启用");out[pos]=0;return;}
    unsigned ph=v.phase;
    append(out,&pos,cap,"状态:");append(out,&pos,cap,ph<8?PHASES[ph]:"?");
    if(v.error){append(out,&pos,cap," 错误:");append_num(out,&pos,cap,v.error<0?0u-(unsigned)v.error:(unsigned)v.error);}
    if(v.passkey){append(out,&pos,cap,"\n配对码:");append_num(out,&pos,cap,v.passkey);}
    if(v.cid){append(out,&pos,cap,"\n链路 CID:");append_num(out,&pos,cap,v.cid);
        append(out,&pos,cap," MTU:");append_num(out,&pos,cap,v.mtu);}
    append(out,&pos,cap,"\n设备 ");append_num(out,&pos,cap,v.count);
    out[pos]=0;
    unsigned count=v.count;if(count>8)count=8;
    for(unsigned i=0;i<count;i++){
        append(out,&pos,cap,"\n[");append_num(out,&pos,cap,i+1);append(out,&pos,cap,"] ");
        append(out,&pos,cap,v.peers[i].device.name);
        append(out,&pos,cap," ");append_mac(out,&pos,cap,v.peers[i].device.address);
    }
    if(ip.ready){
        append(out,&pos,cap,"\nIP ");append_ip(out,&pos,cap,ip.ip);
        append(out,&pos,cap," 掩码 ");append_ip(out,&pos,cap,ip.mask);
        append(out,&pos,cap,"\n网关 ");append_ip(out,&pos,cap,ip.gateway);
        append(out,&pos,cap," DNS ");append_ip(out,&pos,cap,ip.dns);
    } else append(out,&pos,cap,"\nIP 未获取");
    out[pos]=0;
}

/* ===== 平台实现（chaos_web_platform.h 的 Chaos 侧）===== */

/* 账密由表盘页面写入（键盘输入），worker 与 UI 读同一份快照。 */
int chaos_web_credentials(char *username, unsigned user_cap, char *password, unsigned password_cap)
{
    if(!username || !password || !user_cap || !password_cap)return -22;
    username[0]=0;password[0]=0;
    unsigned p=lock();
    unsigned u=0;while(u<31 && web_user[u])u++;
    unsigned w=0;while(w<64 && web_pass[w])w++;
    int rc=have_credential?0:-13;
    if(!rc && u+1<=user_cap && w+1<=password_cap){
        memcpy(username,web_user,u+1);memcpy(password,web_pass,w+1);
    } else if(!rc) rc=-22;
    unlock(p);return rc;
}
/* Rust 键盘页写入; 两个指针都必须指向 NUL 结尾 ASCII。 */
int chaos_net_set_credentials(const char *user, const char *pass)
{
    if(!user || !pass || strncmp(user,"chaos",6)!=0)return -22;
    unsigned w=0;while(w<=31 && pass[w]){if(pass[w]<32 || pass[w]>126)return -22;w++;}
    if(w<5 || w>31 || pass[w]!=0)return -22; /* 默认密码 chaos 为 5 位 */
    unsigned p=lock();
    int changed=!have_credential || strcmp(web_user,"chaos")!=0 || strcmp(web_pass,pass)!=0;
    if(changed){
        /* Epoch invalidates old auth; PAN worker owns transport teardown. */
        chaos_httpd_enable(0);
        memset(web_user,0,sizeof(web_user));memset(web_pass,0,sizeof(web_pass));
        memcpy(web_user,"chaos",6);memcpy(web_pass,pass,w);
        have_credential=1;
    }
    unlock(p);return 0;
}

void chaos_web_sysinfo_read(struct chaos_web_sysinfo *out)
{
    if(!out)return;memset(out,0,sizeof(*out));
    unsigned p;
    p=0;append(out->model,&p,sizeof(out->model),"Xiaomi Smart Band 10 Pro");
    p=0;append(out->firmware,&p,sizeof(out->firmware),"3.101.043");
    p=0;append(out->build,&p,sizeof(out->build),CHAOS_BUILD_ID);
    extern int chaos_network_uptime_seconds(uint32_t *);
    extern int chaos_network_read_meminfo(char *,unsigned);
    out->valid=CHAOS_WEB_CAN_REBOOT;
    if(!chaos_network_uptime_seconds(&out->uptime_seconds))out->valid|=CHAOS_WEB_HAVE_UPTIME;
    int n=chaos_network_read_meminfo(out->meminfo,sizeof(out->meminfo)-1);
    if(n>0){if((unsigned)n>=sizeof(out->meminfo))n=sizeof(out->meminfo)-1;out->meminfo[n]=0;out->valid|=CHAOS_WEB_HAVE_MEMINFO;}
}
int chaos_web_reboot(void){extern int chaos_network_reboot(void);return chaos_network_reboot();}
