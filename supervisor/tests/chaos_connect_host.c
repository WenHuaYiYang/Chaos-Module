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
