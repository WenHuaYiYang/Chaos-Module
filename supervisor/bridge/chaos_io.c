/* Exact p67/3.101.043 VFS entry points. */
#include "chaos_io.h"
#include <stddef.h>
#define NUTTX_O_RDONLY 1u
#define NUTTX_O_WRONLY 2u
#define NUTTX_O_RDWR 3u
#define NUTTX_O_CREAT 4u
#define NUTTX_SEEK_END 2u
#define FW_OPEN        ((uintptr_t)0x0C1D0A29u)
#define FW_CLOSE       ((uintptr_t)0x0C1B9D81u)
#define FW_READ        ((uintptr_t)0x0C1D129Du)
#define FW_WRITE       ((uintptr_t)0x0C1D2641u)
#define FW_LSEEK       ((uintptr_t)0x0C1D0525u)
#define FW_OPENDIR     ((uintptr_t)0x0C1E4529u)
#define FW_READDIR     ((uintptr_t)0x0C1E4591u)
#define FW_CLOSEDIR    ((uintptr_t)0x0C1E4565u)
#define FW_REMOVE      ((uintptr_t)0x0C1EABE1u)
#define FW_MKDIR       ((uintptr_t)0x0C1D05E9u)
#define FW_RENAME      ((uintptr_t)0x0C1D12E9u)
#define FW_STAT        ((uintptr_t)0x0C1D1E9Du)
#define FW_ERRNO       ((uintptr_t)0x0C1E45BDu)

typedef int (*open_fn)(const char *, int, ...);
typedef int (*close_fn)(int);
typedef int32_t (*read_fn)(int, void *, uint32_t);
typedef int32_t (*write_fn)(int, const void *, uint32_t);
typedef int64_t (*lseek_fn)(int, int64_t, int);
typedef void *(*opendir_fn)(const char *);
typedef void *(*readdir_fn)(void *);
typedef int (*closedir_fn)(void *);
typedef int (*remove_fn)(const char *);
typedef int (*mkdir_fn)(const char *, uint32_t);
typedef int (*rename_fn)(const char *, const char *);
typedef int (*stat_fn)(const char *, void *);
typedef int *(*errno_fn)(void);

static int neg_errno(void){int *e=((errno_fn)FW_ERRNO)();return e&&*e>0?-*e:-5;}
static int path_bad(const char *p){if(!p||*p!='/')return 1;while(*p){if((unsigned char)*p++<0x20)return 1;}return 0;}
int32_t cc_io_open(const char *path, uint32_t flags)
{
    if (path_bad(path)) return (-22);
    int access;
    int create = 0;
    if ((flags & CC_IO_WRITE) == 0u) {
        access = NUTTX_O_RDONLY;
    } else {
        access = (flags & CC_IO_READ) != 0u ? NUTTX_O_RDWR : NUTTX_O_WRONLY;
        create = NUTTX_O_CREAT;
        if ((flags & CC_IO_APPEND) == 0u) {
            (void)((remove_fn)FW_REMOVE)(path);
        }
    }
    int fd = ((open_fn)FW_OPEN)(path, (int)(access | create), 0666u);
    return fd < 0 ? neg_errno() : fd;
}

int32_t cc_io_read(int32_t fd, void *buf, uint32_t size)
{
    int32_t n = ((read_fn)FW_READ)(fd, buf, size);
    return n < 0 ? neg_errno() : n;
}

int32_t cc_io_write(int32_t fd, const void *buf, uint32_t size)
{
    int32_t n = ((write_fn)FW_WRITE)(fd, buf, size);
    return n < 0 ? neg_errno() : n;
}

int64_t cc_io_seek(int32_t fd, int64_t off, int32_t whence)
{
    int64_t n = ((lseek_fn)FW_LSEEK)(fd, off, (int)whence);
    return n < 0 ? neg_errno() : n;
}

int64_t cc_io_size(int32_t fd)
{
    int64_t end = cc_io_seek(fd, 0, NUTTX_SEEK_END);
    if (end < 0) return end;
    (void)cc_io_seek(fd, 0, 0);
    return end;
}

int32_t cc_io_close(int32_t fd)
{
    return ((close_fn)FW_CLOSE)(fd) < 0 ? neg_errno() : 0;
}

int32_t cc_io_remove(const char *path)
{
    struct cc_stat st;
    if(cc_io_stat(path,&st)==0&&st.is_dir)return -21;
    return ((remove_fn)FW_REMOVE)(path) < 0 ? neg_errno() : 0;
}

int32_t cc_io_rename(const char *a, const char *b)
{
    return ((rename_fn)FW_RENAME)(a, b) < 0 ? neg_errno() : 0;
}

int32_t cc_io_stat(const char *path,
                      struct cc_stat *out)
{
    uint32_t st[26];
    if (out == 0) return (-22);
    for (uint32_t i = 0; i < 26u; i++) st[i] = 0;
    if (((stat_fn)FW_STAT)(path, st) < 0) return neg_errno();
    out->is_dir = ((st[2] & 0xF000u) == 0x4000u) ? 1u : 0u;
    out->size = (uint32_t)st[8];
    out->mtime = 0;
    return 0;
}

int32_t cc_io_mkdir(const char *path)
{
    return ((mkdir_fn)FW_MKDIR)(path, 0777u) < 0 ? neg_errno() : 0;
}

void *cc_io_opendir(const char *path)
{
    void *d = ((opendir_fn)FW_OPENDIR)(path);
    return d;
}

int32_t cc_io_readdir(void *dir,
                         struct cc_dirent *out)
{
    uint8_t *de = (uint8_t *)((readdir_fn)FW_READDIR)(dir);
    if (de == 0) return 1;
    const char *name = (const char *)(de + 1);
    uint32_t n = 0;
    while (n + 1u < sizeof(out->name) && name[n] != '\0') {
        out->name[n] = name[n];
        n++;
    }
    out->name[n] = '\0';
    out->is_dir = (de[0] == 4u || de[0] == 2u) ? 1u : 0u;
    return 0;
}

int32_t cc_io_closedir(void *dir)
{
    return ((closedir_fn)FW_CLOSEDIR)(dir) < 0 ? neg_errno() : 0;
}

int32_t cc_io_rmdir(const char *path)
{
    struct cc_stat st;int rc=cc_io_stat(path,&st);
    if(rc<0)return rc;
    if(!st.is_dir)return -20;
    return ((remove_fn)FW_REMOVE)(path)<0?neg_errno():0;
}


int32_t cc_io_sync(int32_t fd){(void)fd;return 0;}
