#define CHAOS_NET_GLUE_TEST 1
#include <assert.h>
#include <limits.h>
#include <stdio.h>
#include <string.h>
#include "../network/src/net_glue.c"
static struct network_view mock_network;
static struct pan_ip_info mock_ip;
static int enable_value, enable_calls, uptime_rc, meminfo_rc=6, reboot_calls;
static char old_password[65];
void network_enable(int x){(void)x;}
void network_scan(void){}
void network_connect(unsigned x){(void)x;}
void network_disconnect(void){}
void network_pair_confirm(void){}
void network_service_view(struct network_view *out){if(out)*out=mock_network;}
void pan_ip_info(struct pan_ip_info *out){if(out)*out=mock_ip;}
void chaos_httpd_enable(int on){char user[32];enable_calls++;enable_value=on;(void)chaos_web_credentials(user,sizeof(user),old_password,sizeof(old_password));}
int chaos_network_uptime_seconds(uint32_t *seconds){*seconds=12345;return uptime_rc;}
int chaos_network_read_meminfo(char *out,unsigned n){if(meminfo_rc>0){assert(n>=6);memcpy(out,"memory",6);}return meminfo_rc;}
int chaos_network_reboot(void){reboot_calls++;return -12;}
int main(void){
    char u[32],p[65];memset(u,0xa5,sizeof(u));memset(p,0xa5,sizeof(p));
    assert(chaos_web_credentials(u,sizeof(u),p,sizeof(p))==-13);assert(!u[0]&&!p[0]);
    chaos_httpd_set_enable(1);assert(enable_value==0);
    assert(chaos_net_set_credentials("x","12345678")==-22);
    assert(chaos_net_set_credentials("chaos","1234567")==-22);
    assert(chaos_net_set_credentials("chaos","1234567\n")==-22);
    assert(chaos_net_set_credentials("chaos","12345678901234567890123456789012")==-22);
    assert(chaos_net_set_credentials("chaos","12345678")==0);assert(enable_value==0);
    assert(chaos_web_credentials(u,sizeof(u),p,sizeof(p))==0);assert(!strcmp(u,"chaos")&&!strcmp(p,"12345678"));
    chaos_httpd_set_enable(1);assert(enable_value==1);int n=enable_calls;
    assert(chaos_net_set_credentials("chaos","12345678")==0);assert(enable_calls==n&&enable_value==1);
    assert(chaos_net_set_credentials("chaos","new password")==0);assert(enable_value==0);assert(!strcmp(old_password,"12345678"));
    assert(chaos_web_credentials(u,sizeof(u),p,sizeof(p))==0);assert(!strcmp(p,"new password"));
    assert(chaos_net_set_credentials("chaos","bad")==-22);assert(chaos_web_credentials(u,sizeof(u),p,sizeof(p))==0);assert(!strcmp(p,"new password"));
    assert(chaos_net_set_credentials("chaos","1234567890123456789012345678901")==0);
    assert(chaos_web_credentials(u,1,p,1)==-22);assert(!u[0]&&!p[0]);
    assert(chaos_web_credentials(0,32,p,65)==-22);
    memset(&mock_network,0,sizeof(mock_network));mock_network.enabled=1;mock_network.count=16;mock_network.error=INT_MIN;mock_network.passkey=999999;
    for(unsigned i=0;i<16;i++){memcpy(mock_network.peers[i].device.name,"abcdefghijklmnopqrst",21);memset(mock_network.peers[i].device.address,0xab,6);}
    for(unsigned cap=1;cap<=512;cap++){unsigned char b[516];memset(b,0xa5,sizeof(b));chaos_pan_snapshot((char*)b+2,cap);assert(b[0]==0xa5&&b[1]==0xa5);assert(memchr(b+2,0,cap));for(unsigned i=cap+2;i<sizeof(b);i++)assert(b[i]==0xa5);}
    struct chaos_web_sysinfo info;chaos_web_sysinfo_read(&info);assert(info.valid==(CHAOS_WEB_HAVE_UPTIME|CHAOS_WEB_HAVE_MEMINFO|CHAOS_WEB_CAN_REBOOT));assert(info.uptime_seconds==12345);assert(!strcmp(info.meminfo,"memory"));assert(!(info.valid&CHAOS_WEB_HAVE_HEAP));
    uptime_rc=-5;meminfo_rc=-2;chaos_web_sysinfo_read(&info);assert(info.valid==CHAOS_WEB_CAN_REBOOT);assert(!info.meminfo[0]);
    assert(chaos_web_reboot()==-12&&reboot_calls==1);
    puts("net_glue: credential invalidation, bounded buffers, status availability PASS");
    return 0;
}
