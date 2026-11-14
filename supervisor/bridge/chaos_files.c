#include "chaos_connect.h"
#include "chaos_io.h"
/* Chaos file operations, direct VFS calls, no module ABI. */
#include <stddef.h>
#include <stdint.h>

#define CCF_PROTO_VERSION 1u
#define CCF_MAX_FRAME CC_MAX_PAYLOAD
#define CCF_MAX_PATH 255u
#define CCF_CHUNK CC_CHUNK
#define CCF_PAYLOAD_MAX CC_MAX_PAYLOAD
#define CCF_MAX_ENTRIES 16u
#define CCF_REPORT_STEP 8192u

#define CCF_OP_HELLO 0x01u
#define CCF_OP_LIST 0x02u
#define CCF_OP_STAT 0x03u
#define CCF_OP_MKDIR 0x04u
#define CCF_OP_REMOVE 0x05u
#define CCF_OP_RMDIR 0x06u
#define CCF_OP_RENAME 0x07u
#define CCF_OP_COPY 0x08u
#define CCF_OP_OPEN_READ 0x10u
#define CCF_OP_READ 0x11u
#define CCF_OP_CLOSE_READ 0x12u
#define CCF_OP_OPEN_WRITE 0x20u
#define CCF_OP_WRITE 0x21u
#define CCF_OP_COMMIT 0x22u
#define CCF_OP_ABORT 0x23u
#define CCF_OP_PING 0x30u
#define CCF_OP_SET_DEV 0x40u
#define CCF_OP_GET_DEV 0x41u

#define CCF_ST_OK 0u
#define CCF_ST_BAD 1u
#define CCF_ST_NOTFOUND 2u
#define CCF_ST_ACCESS 3u
#define CCF_ST_EXIST 4u
#define CCF_ST_NOTDIR 5u
#define CCF_ST_ISDIR 6u
#define CCF_ST_NOSPACE 7u
#define CCF_ST_BUSY 8u
#define CCF_ST_IO 9u
#define CCF_ST_CRC 10u
#define CCF_ST_SEQUENCE 11u
#define CCF_ST_UNSUPPORTED 12u
#define CCF_ST_TOO_LARGE 13u

#define CCF_FLAG_DEV_MODE 1u
#define CCF_FLAG_RMDIR 2u

/* No libc imports. Keep all helpers internal; the stock modlib loader only
 * has to apply ABS32/TARGET1 data relocations, not intra-module Thumb calls. */
static size_t ccf_strlen(const char *s)
{
    size_t n = 0;
    while (s[n]) n++;
    return n;
}

static int ccf_strcmp(const char *a, const char *b)
{
    while (*a && *a == *b) { a++; b++; }
    return (unsigned char)*a - (unsigned char)*b;
}

static uint16_t g_chunk=CC_CHUNK;
static uint8_t g_dev_mode;

static int32_t g_read_fd = -1;
static uint32_t g_read_size;
static int32_t g_write_fd = -1;
static char g_write_target[CC_PATH_MAX];
static char g_write_temp[CC_PATH_MAX];
static uint32_t g_write_expected;
static uint32_t g_write_written;
static uint32_t g_write_crc;
static uint32_t g_write_reported;

static uint16_t ccf_get_u16(const uint8_t *p)
{
    return (uint16_t)((uint16_t)p[0] | ((uint16_t)p[1] << 8));
}

static uint32_t ccf_get_u32(const uint8_t *p)
{
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) |
           ((uint32_t)p[3] << 24);
}

static void ccf_put_u16(uint8_t *p, uint16_t v)
{
    p[0] = (uint8_t)v;
    p[1] = (uint8_t)(v >> 8);
}

static void ccf_put_u32(uint8_t *p, uint32_t v)
{
    p[0] = (uint8_t)v;
    p[1] = (uint8_t)(v >> 8);
    p[2] = (uint8_t)(v >> 16);
    p[3] = (uint8_t)(v >> 24);
}

static uint32_t ccf_copy_bytes(uint8_t *dst, const uint8_t *src, uint32_t n)
{
    uint32_t i;
    for (i = 0; i < n; i++) dst[i] = src[i];
    return n;
}

static char ccf_lower(char c)
{
    if (c >= 'A' && c <= 'Z') return (char)(c + ('a' - 'A'));
    return c;
}


static int ccf_under(const char *path, const char *root)
{
    uint32_t i = 0;
    while (root[i]) {
        if (ccf_lower(path[i]) != ccf_lower(root[i])) return 0;
        i++;
    }
    return path[i] == '\0' || path[i] == '/';
}

static int ccf_protected(const char *path)
{
    if (ccf_under(path, "/dev")) return 1;
    if (ccf_under(path, "/proc")) return 1;
    if (ccf_under(path, "/sys")) return 1;
    return 0;
}

static int ccf_normalize(const char *in, char *out)
{
    uint32_t o = 0;
    const char *s = in;
    if (in == 0 || in[0] != '/') return -1;
    while (*s) {
        const char *e;
        uint32_t n;
        while (*s == '/') s++;
        if (*s == '\0') break;
        e = s;
        while (*e && *e != '/') {
            if ((uint8_t)*e < 0x20u) return -1;
            e++;
        }
        n = (uint32_t)(e - s);
        if (n == 1 && s[0] == '.') {
            /* skip */
        } else if (n == 2 && s[0] == '.' && s[1] == '.') {
            if (o == 0) return -1;
            while (o > 0 && out[o - 1] != '/') o--;
            if (o > 0) o--;
        } else {
            if (o + n + 2u >= CC_PATH_MAX) return -1;
            out[o++] = '/';
            {
                uint32_t i;
                for (i = 0; i < n; i++) out[o++] = s[i];
            }
        }
        s = e;
    }
    if (o == 0) out[o++] = '/';
    out[o] = '\0';
    return 0;
}

static int ccf_parse_path(const uint8_t *p, uint32_t plen, uint32_t *used, char *out)
{
    uint16_t n;
    uint32_t i;
    char raw[CC_PATH_MAX];
    if (plen < 2u) return -1;
    n = ccf_get_u16(p);
    if (n == 0u || n >= CC_PATH_MAX || plen < (uint32_t)n + 2u) return -1;
    for (i = 0; i < n; i++) {
        if (p[2u + i] == 0u) return -1;
        raw[i] = (char)p[2u + i];
    }
    raw[n] = '\0';
    if (ccf_normalize(raw, out) != 0) return -1;
    if (used) *used = (uint32_t)n + 2u;
    return 0;
}

static uint8_t ccf_status_from_err(int32_t err)
{
    switch (err) {
    case 0: return CCF_ST_OK;
    case -2: return CCF_ST_NOTFOUND;
    case -13: return CCF_ST_ACCESS;
    case -17: return CCF_ST_EXIST;
    case -20: return CCF_ST_NOTDIR;
    case -21: return CCF_ST_ISDIR;
    case -28: return CCF_ST_NOSPACE;
    case -16:
    case -39: return CCF_ST_BUSY;
    case -38: return CCF_ST_UNSUPPORTED;
    case -22: return CCF_ST_BAD;
    default: return CCF_ST_IO;
    }
}

static int ccf_write_allowed(const char *path)
{
    if (ccf_protected(path)) return 0;
    if (g_dev_mode) return 1;
    return ccf_under(path, "/data");
}

static int ccf_read_allowed(const char *path)
{
    return !ccf_protected(path);
}

static void ccf_send(uint8_t op,uint16_t req,uint8_t status,const uint8_t *payload,uint32_t plen)
{(void)cc_reply(CC_RESPONSE,op,status,req,payload,plen);}
static void ccf_event(uint8_t code,const uint8_t *payload,uint32_t plen)
{(void)cc_reply(CC_EVENT,code,0,0,payload,plen);}
static void ccf_close_read(void){if(g_read_fd>=0)(void)cc_io_close(g_read_fd);g_read_fd=-1;g_read_size=0;}
static void ccf_discard_write(void){
 if(g_write_fd>=0)(void)cc_io_close(g_write_fd);g_write_fd=-1;
 if(g_write_temp[0])(void)cc_io_remove(g_write_temp);
 g_write_temp[0]=g_write_target[0]=0;g_write_expected=g_write_written=g_write_reported=0;g_write_crc=0xffffffffu;
}
void cc_files_reset(void){ccf_close_read();ccf_discard_write();g_dev_mode=0;}
void cc_files_set_chunk(uint16_t chunk){g_chunk=chunk>0&&chunk<=CC_CHUNK?chunk:CC_CHUNK;}
static uint32_t ccf_crc32_update(uint32_t crc, const uint8_t *data, uint32_t len)
{
    uint32_t i, j;
    for (i = 0; i < len; i++) {
        crc ^= data[i];
        for (j = 0; j < 8u; j++) {
            if (crc & 1u) crc = (crc >> 1) ^ 0xEDB88320u;
            else crc >>= 1;
        }
    }
    return crc;
}

static int ccf_temp_for(const char *target, char *out)
{
    uint32_t n = (uint32_t)ccf_strlen(target);
    static uint32_t serial;
    static const char hex[]="0123456789abcdef";
    struct cc_stat st;
    if(n==0||n+21>=CC_PATH_MAX)return -1;
    ccf_copy_bytes((uint8_t *)out,(const uint8_t *)target,n);
    ccf_copy_bytes((uint8_t *)out+n,(const uint8_t *)".cc-",4);
    for(unsigned attempt=0;attempt<32;attempt++){
        uint32_t id=++serial;
        for(unsigned i=0;i<8;i++)out[n+4+i]=hex[(id>>(28-4*i))&15];
        ccf_copy_bytes((uint8_t *)out+n+12,(const uint8_t *)".part",6);
        if(cc_io_stat(out,&st)==-2)return 0;
    }
    return -1;
}

static int ccf_path_join(const char *dir, const char *name, char *out)
{
    uint32_t dl = (uint32_t)ccf_strlen(dir);
    uint32_t nl = (uint32_t)ccf_strlen(name);
    if (dl + nl + 2u >= CC_PATH_MAX) return -1;
    if (dl == 1u && dir[0] == '/') {
        out[0] = '/';
        ccf_copy_bytes((uint8_t *)out + 1, (const uint8_t *)name, nl);
        out[nl + 1] = '\0';
    } else {
        ccf_copy_bytes((uint8_t *)out, (const uint8_t *)dir, dl);
        out[dl] = '/';
        ccf_copy_bytes((uint8_t *)out + dl + 1, (const uint8_t *)name, nl);
        out[dl + nl + 1] = '\0';
    }
    return 0;
}

static void ccf_handle_hello(uint16_t req)
{
    uint8_t out[16];
    uint32_t n = 0;
    out[n++] = CCF_PROTO_VERSION;
    ccf_put_u16(&out[n], g_chunk); n += 2u;
    out[n++] = (uint8_t)(CCF_FLAG_DEV_MODE | CCF_FLAG_RMDIR);
    out[n++] = g_dev_mode;
    ccf_put_u16(&out[n], (uint16_t)CCF_MAX_PATH); n += 2u;
    ccf_put_u32(&out[n], CC_WINDOW); n += 4u;
    ccf_send(CCF_OP_HELLO, req, CCF_ST_OK, out, n);
}

static void ccf_handle_list(uint16_t req,
                           const uint8_t *p, uint32_t plen)
{
    char path[CC_PATH_MAX];
    uint32_t used = 0, offset, idx = 0, count = 0, total = 0;
    uint8_t limit, page_full=0;
    static uint8_t out[CCF_MAX_FRAME];
    uint32_t o = 0;
    void *dir;
    if (ccf_parse_path(p, plen, &used, path) != 0 || plen < used + 5u) {
        ccf_send(CCF_OP_LIST, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (!ccf_read_allowed(path)) {
        ccf_send(CCF_OP_LIST, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    offset = ccf_get_u32(p + used);
    limit = p[used + 4u];
    if(!limit){ccf_send(CCF_OP_LIST,req,CCF_ST_BAD,0,0);return;}
    if (limit > CCF_MAX_ENTRIES) limit = CCF_MAX_ENTRIES;
    dir = cc_io_opendir(path);
    if (dir == 0) {
        ccf_send(CCF_OP_LIST, req, CCF_ST_NOTFOUND, 0, 0);
        return;
    }
    o = 6u; /* total/offset/count placeholder */
    while (1) {
        struct cc_dirent ent;
        int32_t rc = cc_io_readdir(dir, &ent);
        char full[CC_PATH_MAX];
        struct cc_stat st;
        uint32_t nl;
        uint32_t need;
        if (rc == 1) break;
        if (rc < 0) {
            (void)cc_io_closedir(dir);
            ccf_send(CCF_OP_LIST, req, ccf_status_from_err(rc), 0, 0);
            return;
        }
        if (ent.name[0] == '.' && (ent.name[1] == '\0' ||
            (ent.name[1] == '.' && ent.name[2] == '\0'))) continue;
        nl = (uint32_t)ccf_strlen(ent.name);
        total++;
        if (idx++ < offset) continue;
        if (count >= limit || page_full) continue;
        need = 10u + nl;
        if (o + need > g_chunk) {page_full=1;continue;}
        st.size = 0;
        st.mtime = 0;
        st.is_dir = ent.is_dir;
        if (ccf_path_join(path, ent.name, full) == 0) {
            struct cc_stat tmp;
            if (cc_io_stat(full, &tmp) == 0) {
                st.size = tmp.size;
                st.mtime = tmp.mtime;
                st.is_dir = tmp.is_dir;
            }
        }
        out[o++] = (uint8_t)nl;
        ccf_copy_bytes(&out[o], (const uint8_t *)ent.name, nl); o += nl;
        out[o++] = (uint8_t)(st.is_dir ? 1u : 0u);
        ccf_put_u32(&out[o], (uint32_t)st.size); o += 4u;
        ccf_put_u32(&out[o], (uint32_t)st.mtime); o += 4u;
        count++;
    }
    (void)cc_io_closedir(dir);
    if(!count&&total>offset){ccf_send(CCF_OP_LIST,req,CCF_ST_TOO_LARGE,0,0);return;}
    ccf_put_u16(&out[0], (uint16_t)(total > 0xFFFFu ? 0xFFFFu : total));
    ccf_put_u16(&out[2], (uint16_t)(offset > 0xFFFFu ? 0xFFFFu : offset));
    ccf_put_u16(&out[4], (uint16_t)count);
    ccf_send(CCF_OP_LIST, req, CCF_ST_OK, out, o);
}

static void ccf_handle_stat(uint16_t req,
                           const uint8_t *p, uint32_t plen)
{
    char path[CC_PATH_MAX];
    struct cc_stat st;
    uint8_t out[9];
    int32_t rc;
    if (ccf_parse_path(p, plen, 0, path) != 0) {
        ccf_send(CCF_OP_STAT, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (!ccf_read_allowed(path)) {
        ccf_send(CCF_OP_STAT, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    rc = cc_io_stat(path, &st);
    if (rc < 0) {
        ccf_send(CCF_OP_STAT, req, ccf_status_from_err(rc), 0, 0);
        return;
    }
    out[0] = (uint8_t)(st.is_dir ? 1u : 0u);
    ccf_put_u32(&out[1], (uint32_t)st.size);
    ccf_put_u32(&out[5], (uint32_t)st.mtime);
    ccf_send(CCF_OP_STAT, req, CCF_ST_OK, out, 9);
}

static void ccf_handle_mkdir(uint16_t req,
                            const uint8_t *p, uint32_t plen)
{
    char path[CC_PATH_MAX];
    int32_t rc;
    if (ccf_parse_path(p, plen, 0, path) != 0) {
        ccf_send(CCF_OP_MKDIR, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (!ccf_write_allowed(path)) {
        ccf_send(CCF_OP_MKDIR, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    rc = cc_io_mkdir(path);
    ccf_send(CCF_OP_MKDIR, req, rc < 0 ? ccf_status_from_err(rc) : CCF_ST_OK, 0, 0);
}

static void ccf_handle_remove(uint16_t req,
                             const uint8_t *p, uint32_t plen, uint8_t op)
{
    char path[CC_PATH_MAX];
    int32_t rc;
    if (ccf_parse_path(p, plen, 0, path) != 0) {
        ccf_send(op, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (!ccf_write_allowed(path)) {
        ccf_send(op, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    if (op == CCF_OP_RMDIR) {
        rc = cc_io_rmdir(path);
    } else {
        rc = cc_io_remove(path);
    }
    ccf_send(op, req, rc < 0 ? ccf_status_from_err(rc) : CCF_ST_OK, 0, 0);
}

static void ccf_handle_rename(uint16_t req,
                             const uint8_t *p, uint32_t plen)
{
    char from[CC_PATH_MAX];
    char to[CC_PATH_MAX];
    uint32_t used = 0;
    int32_t rc;
    if (ccf_parse_path(p, plen, &used, from) != 0 || used >= plen ||
        ccf_parse_path(p + used, plen - used, 0, to) != 0) {
        ccf_send(CCF_OP_RENAME, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (!ccf_write_allowed(from) || !ccf_write_allowed(to)) {
        ccf_send(CCF_OP_RENAME, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    rc = cc_io_rename(from, to);
    ccf_send(CCF_OP_RENAME, req, rc < 0 ? ccf_status_from_err(rc) : CCF_ST_OK, 0, 0);
}

static void ccf_handle_copy(uint16_t req,
                          const uint8_t *p, uint32_t plen)
{
    char from[CC_PATH_MAX];
    char to[CC_PATH_MAX];
    char temp[CC_PATH_MAX];
    struct cc_stat st;
    uint32_t first = 0, second = 0;
    uint8_t overwrite;
    int32_t src = -1, dst = -1, rc;
    uint8_t buf[512];
    if (ccf_parse_path(p, plen, &first, from) != 0 ||
        ccf_parse_path(p + first, plen - first, &second, to) != 0 ||
        plen < first + second + 1u) {
        ccf_send(CCF_OP_COPY, req, CCF_ST_BAD, 0, 0);
        return;
    }
    overwrite = p[first + second];
    if (!ccf_read_allowed(from) || !ccf_write_allowed(to)) {
        ccf_send(CCF_OP_COPY, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    if (ccf_strcmp(from, to)) {
        /* Different paths, continue. */
    } else {
        ccf_send(CCF_OP_COPY, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (cc_io_stat(from, &st) < 0) {
        ccf_send(CCF_OP_COPY, req, CCF_ST_NOTFOUND, 0, 0);
        return;
    }
    if (st.is_dir) {
        ccf_send(CCF_OP_COPY, req, CCF_ST_ISDIR, 0, 0);
        return;
    }
    if (cc_io_stat(to, &st) == 0) {
        if (st.is_dir) {
            ccf_send(CCF_OP_COPY, req, CCF_ST_ISDIR, 0, 0);
            return;
        }
        if (!overwrite) {
            ccf_send(CCF_OP_COPY, req, CCF_ST_EXIST, 0, 0);
            return;
        }
    }
    if (ccf_temp_for(to, temp) != 0) {
        ccf_send(CCF_OP_COPY, req, CCF_ST_BAD, 0, 0);
        return;
    }
    (void)cc_io_remove(temp);
    src = cc_io_open(from, CC_IO_READ);
    if (src < 0) {
        ccf_send(CCF_OP_COPY, req, ccf_status_from_err(src), 0, 0);
        return;
    }
    dst = cc_io_open(temp, CC_IO_WRITE | CC_IO_CREATE | CC_IO_TRUNC);
    if (dst < 0) {
        (void)cc_io_close(src);
        ccf_send(CCF_OP_COPY, req, ccf_status_from_err(dst), 0, 0);
        return;
    }
    for (;;) {
        int32_t got = cc_io_read(src, buf, sizeof(buf));
        uint32_t sent = 0;
        if (got < 0) {
            rc = got;
            goto fail;
        }
        if (got == 0) break;
        while (sent < (uint32_t)got) {
            int32_t wrote = cc_io_write(dst, buf + sent, (uint32_t)got - sent);
            if (wrote <= 0) {
                rc = wrote < 0 ? wrote : -5;
                goto fail;
            }
            sent += (uint32_t)wrote;
        }
    }
    (void)cc_io_sync(dst);
    rc = cc_io_close(dst);
    dst = -1;
    if (rc < 0) goto fail;
    (void)cc_io_close(src);
    src = -1;
    rc = cc_io_rename(temp, to);
    if (rc < 0) {
        (void)cc_io_remove(temp);
        ccf_send(CCF_OP_COPY, req, ccf_status_from_err(rc), 0, 0);
        return;
    }
    ccf_send(CCF_OP_COPY, req, CCF_ST_OK, 0, 0);
    return;
fail:
    if (dst >= 0) (void)cc_io_close(dst);
    if (src >= 0) (void)cc_io_close(src);
    (void)cc_io_remove(temp);
    ccf_send(CCF_OP_COPY, req, ccf_status_from_err(rc), 0, 0);
}

static void ccf_handle_open_read(uint16_t req,
                                const uint8_t *p, uint32_t plen)
{
    char path[CC_PATH_MAX];
    uint8_t out[4];
    int32_t fd;
    int64_t size;
    if (ccf_parse_path(p, plen, 0, path) != 0) {
        ccf_send(CCF_OP_OPEN_READ, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (!ccf_read_allowed(path)) {
        ccf_send(CCF_OP_OPEN_READ, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    ccf_close_read();
    fd = cc_io_open(path, CC_IO_READ);
    if (fd < 0) {
        ccf_send(CCF_OP_OPEN_READ, req, ccf_status_from_err(fd), 0, 0);
        return;
    }
    size = cc_io_size(fd);
    if (size < 0) {
        (void)cc_io_close(fd);
        ccf_send(CCF_OP_OPEN_READ, req, ccf_status_from_err((int32_t)size), 0, 0);
        return;
    }
    g_read_fd = fd;
    g_read_size = (uint32_t)size;
    ccf_put_u32(out, g_read_size);
    ccf_send(CCF_OP_OPEN_READ, req, CCF_ST_OK, out, 4);
}

static void ccf_handle_read(uint16_t req,
                           const uint8_t *p, uint32_t plen)
{
    static uint8_t out[CCF_CHUNK];
    uint32_t offset, want;
    int32_t rc;
    if (g_read_fd < 0 || plen < 6u) {
        ccf_send(CCF_OP_READ, req, CCF_ST_BAD, 0, 0);
        return;
    }
    offset = ccf_get_u32(p);
    want = ccf_get_u16(p + 4);
    if (want > g_chunk) want = g_chunk;
    if (offset >= g_read_size) {
        ccf_send(CCF_OP_READ, req, CCF_ST_OK, 0, 0);
        return;
    }
    if (want > g_read_size - offset) want = g_read_size - offset;
    if (cc_io_seek(g_read_fd, (int64_t)offset, 0) < 0) {
        ccf_send(CCF_OP_READ, req, CCF_ST_IO, 0, 0);
        return;
    }
    {
        uint32_t got = 0;
        while (got < want) {
            rc = cc_io_read(g_read_fd, out + got, want - got);
            if (rc < 0) {
                ccf_send(CCF_OP_READ, req, ccf_status_from_err(rc), 0, 0);
                return;
            }
            if (rc == 0) break;
            got += (uint32_t)rc;
        }
        ccf_send(CCF_OP_READ, req, CCF_ST_OK, out, got);
    }
}

static void ccf_handle_open_write(uint16_t req,
                                 const uint8_t *p, uint32_t plen)
{
    char path[CC_PATH_MAX];
    char temp[CC_PATH_MAX];
    uint32_t used = 0, expected;
    uint8_t overwrite;
    struct cc_stat st;
    int32_t fd;
    if (ccf_parse_path(p, plen, &used, path) != 0 || plen < used + 5u) {
        ccf_send(CCF_OP_OPEN_WRITE, req, CCF_ST_BAD, 0, 0);
        return;
    }
    expected = ccf_get_u32(p + used);
    overwrite = p[used + 4u];
    if(expected>32u*1024u*1024u){ccf_send(CCF_OP_OPEN_WRITE,req,CCF_ST_TOO_LARGE,0,0);return;}
    if (!ccf_write_allowed(path)) {
        ccf_send(CCF_OP_OPEN_WRITE, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    if (cc_io_stat(path, &st) == 0 && st.is_dir) {
        ccf_send(CCF_OP_OPEN_WRITE, req, CCF_ST_ISDIR, 0, 0);
        return;
    }
    if (!overwrite && cc_io_stat(path, &st) == 0) {
        ccf_send(CCF_OP_OPEN_WRITE, req, CCF_ST_EXIST, 0, 0);
        return;
    }
    if (ccf_temp_for(path, temp) != 0) {
        ccf_send(CCF_OP_OPEN_WRITE, req, CCF_ST_BAD, 0, 0);
        return;
    }
    ccf_discard_write();
    (void)cc_io_remove(temp);
    fd = cc_io_open(temp, CC_IO_WRITE | CC_IO_CREATE | CC_IO_TRUNC);
    if (fd < 0) {
        ccf_send(CCF_OP_OPEN_WRITE, req, ccf_status_from_err(fd), 0, 0);
        return;
    }
    g_write_fd = fd;
    ccf_copy_bytes((uint8_t *)g_write_target, (const uint8_t *)path, (uint32_t)ccf_strlen(path) + 1u);
    ccf_copy_bytes((uint8_t *)g_write_temp, (const uint8_t *)temp, (uint32_t)ccf_strlen(temp) + 1u);
    g_write_expected = expected;
    g_write_written = 0;
    g_write_reported = 0;
    g_write_crc = 0xFFFFFFFFu;
    ccf_send(CCF_OP_OPEN_WRITE, req, CCF_ST_OK, 0, 0);
}

static void ccf_handle_write(uint16_t req,
                            const uint8_t *p, uint32_t plen)
{
    uint32_t offset, n;
    int32_t rc;
    if (g_write_fd < 0 || plen < 4u) {
        ccf_send(CCF_OP_WRITE, req, CCF_ST_BAD, 0, 0);
        return;
    }
    offset = ccf_get_u32(p);
    n = plen - 4u;
    if(n>g_chunk){ccf_send(CCF_OP_WRITE,req,CCF_ST_TOO_LARGE,0,0);return;}
    if (offset != g_write_written) {
        ccf_send(CCF_OP_WRITE, req, CCF_ST_SEQUENCE, 0, 0);
        return;
    }
    if (g_write_written > g_write_expected ||
        n > g_write_expected - g_write_written) {
        ccf_send(CCF_OP_WRITE, req, CCF_ST_TOO_LARGE, 0, 0);
        return;
    }
    {
        uint32_t sent = 0;
        while (sent < n) {
            rc = cc_io_write(g_write_fd, p + 4 + sent, n - sent);
            if (rc <= 0) {
                uint8_t error=rc<0?ccf_status_from_err(rc):CCF_ST_IO;
                ccf_discard_write();
                ccf_send(CCF_OP_WRITE, req, error, 0, 0);
                return;
            }
            sent += (uint32_t)rc;
        }
    }
    g_write_crc = ccf_crc32_update(g_write_crc, p + 4, n);
    g_write_written += n;
    if (g_write_written - g_write_reported >= CCF_REPORT_STEP ||
        g_write_written == g_write_expected) {
        uint8_t ev[8];
        ccf_put_u32(&ev[0], g_write_written);
        ccf_put_u32(&ev[4], g_write_crc ^ 0xFFFFFFFFu);
        ccf_event(0xC1u, ev, 8u);
        g_write_reported = g_write_written;
    }
    uint8_t ack[8];ccf_put_u32(ack,g_write_written);ccf_put_u32(ack+4,g_write_crc^0xffffffffu);
    ccf_send(CCF_OP_WRITE, req, CCF_ST_OK, ack, sizeof(ack));
}

static void ccf_handle_commit(uint16_t req,
                             const uint8_t *p, uint32_t plen)
{
    uint32_t expected_crc;
    int32_t rc;
    if (g_write_fd < 0 || plen < 4u) {
        ccf_send(CCF_OP_COMMIT, req, CCF_ST_BAD, 0, 0);
        return;
    }
    expected_crc = ccf_get_u32(p);
    if (g_write_written != g_write_expected) {
        ccf_send(CCF_OP_COMMIT, req, CCF_ST_IO, 0, 0);
        return;
    }
    if ((g_write_crc ^ 0xFFFFFFFFu) != expected_crc) {
        ccf_discard_write();
        ccf_send(CCF_OP_COMMIT, req, CCF_ST_CRC, 0, 0);
        return;
    }
    (void)cc_io_sync(g_write_fd);
    rc = cc_io_close(g_write_fd);
    g_write_fd = -1;
    if (rc < 0) {
        ccf_discard_write();
        ccf_send(CCF_OP_COMMIT, req, ccf_status_from_err(rc), 0, 0);
        return;
    }
    rc = cc_io_rename(g_write_temp, g_write_target);
    if (rc < 0) {
        ccf_discard_write();
        ccf_send(CCF_OP_COMMIT, req, ccf_status_from_err(rc), 0, 0);
        return;
    }
    g_write_temp[0] = '\0';
    g_write_target[0] = '\0';
    g_write_expected = 0;
    g_write_written = 0;
    g_write_crc = 0xFFFFFFFFu;
    g_write_reported = 0;
    ccf_send(CCF_OP_COMMIT, req, CCF_ST_OK, 0, 0);
}

static void ccf_handle_set_dev(uint16_t req,
                              const uint8_t *p, uint32_t plen)
{
    if (plen < 1u) {
        ccf_send(CCF_OP_SET_DEV, req, CCF_ST_BAD, 0, 0);
        return;
    }
    g_dev_mode = p[0] ? 1u : 0u;
    ccf_send(CCF_OP_SET_DEV, req, CCF_ST_OK, 0, 0);
}

void cc_files_dispatch(const struct cc_frame *frame)
{
 uint8_t op=frame->op;uint16_t req=frame->req;
 const uint8_t *p=frame->payload;uint32_t plen=frame->len;
 if(frame->flags!=0||frame->status!=0)return;
    switch (op) {
    case CCF_OP_HELLO: cc_files_reset();ccf_handle_hello(req); break;
    case CCF_OP_LIST: ccf_handle_list(req, p, plen); break;
    case CCF_OP_STAT: ccf_handle_stat(req, p, plen); break;
    case CCF_OP_MKDIR: ccf_handle_mkdir(req, p, plen); break;
    case CCF_OP_REMOVE: ccf_handle_remove(req, p, plen, op); break;
    case CCF_OP_RMDIR: ccf_handle_remove(req, p, plen, op); break;
    case CCF_OP_RENAME: ccf_handle_rename(req, p, plen); break;
    case CCF_OP_COPY: ccf_handle_copy(req, p, plen); break;
    case CCF_OP_OPEN_READ: ccf_handle_open_read(req, p, plen); break;
    case CCF_OP_READ: ccf_handle_read(req, p, plen); break;
    case CCF_OP_CLOSE_READ:
        ccf_close_read();
        ccf_send(op, req, CCF_ST_OK, 0, 0);
        break;
    case CCF_OP_OPEN_WRITE: ccf_handle_open_write(req, p, plen); break;
    case CCF_OP_WRITE: ccf_handle_write(req, p, plen); break;
    case CCF_OP_COMMIT: ccf_handle_commit(req, p, plen); break;
    case CCF_OP_ABORT:
        ccf_discard_write();
        ccf_send(op, req, CCF_ST_OK, 0, 0);
        break;
    case CCF_OP_PING: {
        uint8_t one = 1u;
        ccf_send(op, req, CCF_ST_OK, &one, 1u);
        break;
    }
    case 0x31: /* Link benchmark: echo actual bytes through the full encrypted carrier. */
        ccf_send(op,req,plen>g_chunk?CCF_ST_TOO_LARGE:CCF_ST_OK,plen>g_chunk?0:p,plen>g_chunk?0:plen);
        break;
    case CCF_OP_SET_DEV: ccf_handle_set_dev(req, p, plen); break;
    case CCF_OP_GET_DEV: {
        uint8_t one = g_dev_mode;
        ccf_send(op, req, CCF_ST_OK, &one, 1u);
        break;
    }
    default:
        ccf_send(op, req, CCF_ST_BAD, 0, 0);
        break;
    }
}
