#ifndef CHAOS_IO_H
#define CHAOS_IO_H
#include <stdint.h>
#define CC_PATH_MAX 256u
#define CC_IO_READ 1u
#define CC_IO_WRITE 2u
#define CC_IO_APPEND 4u
#define CC_IO_CREATE 8u
#define CC_IO_TRUNC 16u
struct cc_stat {uint32_t size;uint8_t is_dir;uint32_t mtime;};
struct cc_dirent {char name[256];uint8_t is_dir;};
int32_t cc_io_open(const char *,uint32_t);
int32_t cc_io_read(int32_t,void *,uint32_t);
int32_t cc_io_write(int32_t,const void *,uint32_t);
int64_t cc_io_seek(int32_t,int64_t,int32_t);
int64_t cc_io_size(int32_t);
int32_t cc_io_close(int32_t);
int32_t cc_io_remove(const char *);
int32_t cc_io_rename(const char *,const char *);
int32_t cc_io_stat(const char *,struct cc_stat *);
int32_t cc_io_mkdir(const char *);
int32_t cc_io_rmdir(const char *);
void *cc_io_opendir(const char *);
int32_t cc_io_readdir(void *,struct cc_dirent *);
int32_t cc_io_closedir(void *);
int32_t cc_io_sync(int32_t);
#endif
