/* Production ChaosConnect file engine against a real temporary POSIX VFS.
 * Fault controls cover short writes and rename failure; not radio emulation. */
#include "chaos_connect.h"
#include "chaos_io.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include <fcntl.h>
#include <dirent.h>
#include <errno.h>
static uint8_t replies[32][CC_MAX_FRAME];static unsigned lengths[32],used;
static char logs[256][1024];static unsigned log_count;
static uint32_t service_enabled=1;
uint32_t chaos_bridge_enabled(void){return service_enabled;}
void cc_test_service(uint32_t enabled){service_enabled=enabled;}
static uint32_t android_policy=2,android_mask=1,plugin_policy=0,plugin_mask=31;
uint32_t chaos_connect_plugin_policy(void){return plugin_policy;}
uint32_t chaos_connect_plugin_mask(void){return plugin_mask;}
void cc_test_plugin(uint32_t policy,uint32_t mask){plugin_policy=policy;plugin_mask=mask;}
uint32_t chaos_connect_android_policy(void){return android_policy;}
uint32_t chaos_connect_android_mask(void){return android_mask;}
void cc_test_android(uint32_t forced,uint32_t pol,uint32_t mask){cc_files_set_android(forced);android_policy=pol;android_mask=mask;}
static uint32_t policy,now_ms=100,decision_token,decision,state_flags,state_token;
static uint32_t install_policy=2;
uint32_t chaos_connect_install_policy(void){return install_policy;}
void cc_test_install_policy(uint32_t v){install_policy=v;}
void chaos_connect_install_name(const uint8_t *p,uint32_t n){(void)p;(void)n;}
uint32_t chaos_connect_policy(void){return policy;}
uint32_t chaos_connect_now_ms(void){return now_ms;}
uint32_t chaos_connect_take_decision(uint32_t token){uint32_t d=decision;decision=0;return token==decision_token?d:0;}
void chaos_connect_state(uint32_t flags,uint32_t token){state_flags=flags;state_token=token;}
void cc_test_policy(uint32_t value){policy=value;}
void cc_test_decide(uint32_t token,uint32_t value){decision_token=token;decision=value;}
void cc_test_time(uint32_t value){now_ms=value;}
uint32_t cc_test_state(void){return state_flags;}
uint32_t cc_test_token(void){return state_token;}
void chaos_connect_log(const char *message){if(log_count<256)snprintf(logs[log_count++],sizeof(logs[0]),"%s",message);}
void cc_test_clear_logs(void){log_count=0;}
int cc_test_log(unsigned i,char *out){if(i>=log_count)return 0;strcpy(out,logs[i]);return (int)strlen(out);}
static int short_write,rename_error,write_budget;
void cc_test_fault(int short_bytes,int rename_errno){short_write=short_bytes;rename_error=rename_errno;write_budget=-short_bytes;}
void cc_test_clear(void){used=0;}
int cc_reply(uint8_t flags,uint8_t op,uint8_t status,uint16_t req,const void *p,uint32_t n){
 if(used>=32)abort();int size=cc_encode(replies[used],CC_MAX_FRAME,flags,op,status,req,p,n);if(size<0)abort();lengths[used++]=size;return 0;
}
int cc_test_reply(unsigned i,void *out){if(i>=used)return 0;memcpy(out,replies[i],lengths[i]);return lengths[i];}
int cc_test_dispatch(const uint8_t *p,unsigned n){struct cc_frame f;used=0;int rc=cc_decode(p,n,&f);if(rc)return rc;cc_files_dispatch(&f);return 0;}
static const char *path(const char *name,char out[4096]){snprintf(out,4096,"%s%s",getenv("CHAOS_CONNECT_TEST_ROOT"),name);return out;}
int32_t cc_io_open(const char *name,uint32_t flags){char b[4096];int f=flags&CC_IO_WRITE?O_WRONLY|O_CREAT|O_TRUNC:O_RDONLY;int fd=open(path(name,b),f,0600);return fd<0?-errno:fd;}
int32_t cc_io_read(int32_t fd,void *p,uint32_t n){ssize_t r=read(fd,p,n);return r<0?-errno:r;}
int32_t cc_io_write(int32_t fd,const void *p,uint32_t n){if(short_write>0&&n>(unsigned)short_write)n=short_write;if(short_write<0){if(!write_budget)return -5;if(n>(unsigned)write_budget)n=write_budget;write_budget-=n;}ssize_t r=write(fd,p,n);return r<0?-errno:r;}
int64_t cc_io_seek(int32_t fd,int64_t offset,int32_t whence){off_t r=lseek(fd,offset,whence);return r<0?-errno:r;}
int64_t cc_io_size(int32_t fd){struct stat st;return fstat(fd,&st)?-errno:st.st_size;}
int32_t cc_io_close(int32_t fd){return close(fd)?-errno:0;}
int32_t cc_io_sync(int32_t fd){return fsync(fd)?-errno:0;}
int32_t cc_io_remove(const char *name){char p[4096];return remove(path(name,p))?-errno:0;}
int32_t cc_io_rename(const char *a,const char *b){char p[4096],q[4096];if(rename_error)return -rename_error;return rename(path(a,p),path(b,q))?-errno:0;}
int32_t cc_io_stat(const char *name,struct cc_stat *out){char p[4096];struct stat st;if(stat(path(name,p),&st))return -errno;out->size=st.st_size;out->is_dir=S_ISDIR(st.st_mode);out->mtime=st.st_mtime;return 0;}
int32_t cc_io_mkdir(const char *name){char p[4096];return mkdir(path(name,p),0700)?-errno:0;}
int32_t cc_io_rmdir(const char *name){char p[4096];return rmdir(path(name,p))?-(errno==ENOTEMPTY?39:errno):0;}
void *cc_io_opendir(const char *name){char p[4096];return opendir(path(name,p));}
int32_t cc_io_readdir(void *d,struct cc_dirent *out){errno=0;struct dirent *e=readdir(d);if(!e)return errno?-errno:1;snprintf(out->name,sizeof(out->name),"%s",e->d_name);out->is_dir=e->d_type==DT_DIR;return 0;}
int32_t cc_io_closedir(void *d){return closedir(d)?-errno:0;}

static int terminal_state,terminal_error,terminal_start_count,terminal_stop_count,terminal_write_count;
static uint8_t terminal_bytes[8192];static uint32_t terminal_size;
int32_t chaos_connect_terminal_start(void){++terminal_start_count;terminal_state=2;terminal_size=0;return 0;}
int32_t chaos_connect_terminal_state(void){return terminal_state;}
int32_t chaos_connect_terminal_error(void){return terminal_error;}
int32_t chaos_connect_terminal_send(const uint8_t *p,uint32_t n){
 if(n>2048 || n>sizeof(terminal_bytes)-terminal_size)return -16;
 for(uint32_t i=0;i<n;i++)if(!p[i]||p[i]==4)return -22;
 ++terminal_write_count;memcpy(terminal_bytes+terminal_size,p,n);terminal_size+=n;return 0;
}
uint32_t chaos_connect_terminal_receive(uint8_t *p,uint32_t n){
 if(n>terminal_size)n=terminal_size;memcpy(p,terminal_bytes,n);terminal_size-=n;memmove(terminal_bytes,terminal_bytes+n,terminal_size);return n;
}
void chaos_connect_terminal_stop(void){++terminal_stop_count;terminal_state=5;terminal_size=0;}
uint32_t cc_test_terminal_counter(uint32_t what){return what==0?terminal_start_count:what==1?terminal_stop_count:terminal_write_count;}
/* Control mailbox stand-in: only the production C authorization/dispatch is under
 * test here. Native getters/setters and UI-owner execution are not emulated. */
static uint32_t control_calls,control_token,control_resets;
void chaos_control_reset(void){control_token=0;control_resets++;}
uint32_t chaos_control_submit(uint32_t op,const uint8_t *p,uint32_t n,uint32_t *token){
 if((op==0x70&&n!=2&&n!=3)||((op==0x71||op==0x73)&&(n<4||(uint32_t)(p[2]|p[3]<<8)!=n-4)))return 1;
 control_calls++;control_token=control_calls;*token=control_token;return 0;
}
int32_t chaos_control_result(uint32_t token,uint8_t *p,uint32_t cap){
 if(cap<2)return -1;if(!token||token!=control_token)return -11;p[0]=1;p[1]=0;return 2;
}
uint32_t cc_test_control_counter(uint32_t what){return what==0?control_calls:control_resets;}
