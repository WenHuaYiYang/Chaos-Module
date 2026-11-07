/*
 * band_files — Canopus module that bridges the watch filesystem to an
 * AstroBox plugin over CanopusConnect.
 *
 * Target: Xiaomi Band 10 Pro 3.101.043 (stock modlib). No libc imports.
 *
 * Default write policy: /data (excluding /data/canopus).
 * Developer mode: every absolute path except Canopus' own state and the
 * virtual /dev, /proc, /sys trees.
 */
#include "canopus_abi.h"
#include "canopus_services.h"

#include <stddef.h>
#include <stdint.h>

#define BF_PROTO_VERSION 1u
#define BF_MAX_FRAME 480u
#define BF_MAX_PATH 255u
#define BF_CHUNK 448u
#define BF_PAYLOAD_MAX (BF_MAX_FRAME - 4u)
#define BF_MAX_ENTRIES 16u
#define BF_REPORT_STEP 8192u

#define BF_OP_HELLO 0x01u
#define BF_OP_LIST 0x02u
#define BF_OP_STAT 0x03u
#define BF_OP_MKDIR 0x04u
#define BF_OP_REMOVE 0x05u
#define BF_OP_RMDIR 0x06u
#define BF_OP_RENAME 0x07u
#define BF_OP_COPY 0x08u
#define BF_OP_OPEN_READ 0x10u
#define BF_OP_READ 0x11u
#define BF_OP_CLOSE_READ 0x12u
#define BF_OP_OPEN_WRITE 0x20u
#define BF_OP_WRITE 0x21u
#define BF_OP_COMMIT 0x22u
#define BF_OP_ABORT 0x23u
#define BF_OP_PING 0x30u
#define BF_OP_SET_DEV 0x40u
#define BF_OP_GET_DEV 0x41u

#define BF_ST_OK 0u
#define BF_ST_BAD 1u
#define BF_ST_NOTFOUND 2u
#define BF_ST_ACCESS 3u
#define BF_ST_EXIST 4u
#define BF_ST_NOTDIR 5u
#define BF_ST_ISDIR 6u
#define BF_ST_NOSPACE 7u
#define BF_ST_BUSY 8u
#define BF_ST_IO 9u
#define BF_ST_CRC 10u
#define BF_ST_SEQUENCE 11u
#define BF_ST_UNSUPPORTED 12u
#define BF_ST_TOO_LARGE 13u

#define BF_FLAG_DEV_MODE 1u
#define BF_FLAG_RMDIR 2u

/* No libc imports. Keep all helpers internal; the stock modlib loader only
 * has to apply ABS32/TARGET1 data relocations, not intra-module Thumb calls. */
static size_t bf_strlen(const char *s)
{
    size_t n = 0;
    while (s[n]) n++;
    return n;
}

static int bf_strcmp(const char *a, const char *b)
{
    while (*a && *a == *b) { a++; b++; }
    return (unsigned char)*a - (unsigned char)*b;
}

static const struct canopus_context_v1 *g_ctx;
static uint8_t g_dev_mode;

static int32_t g_read_fd = -1;
static uint32_t g_read_size;
static int32_t g_write_fd = -1;
static char g_write_target[CANOPUS_FS_PATH_MAX];
static char g_write_temp[CANOPUS_FS_PATH_MAX];
static uint32_t g_write_expected;
static uint32_t g_write_written;
static uint32_t g_write_crc;
static uint32_t g_write_reported;
static uint8_t g_write_overwrite;

static uint16_t bf_get_u16(const uint8_t *p)
{
    return (uint16_t)((uint16_t)p[0] | ((uint16_t)p[1] << 8));
}

static uint32_t bf_get_u32(const uint8_t *p)
{
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) |
           ((uint32_t)p[3] << 24);
}

static void bf_put_u16(uint8_t *p, uint16_t v)
{
    p[0] = (uint8_t)v;
    p[1] = (uint8_t)(v >> 8);
}

static void bf_put_u32(uint8_t *p, uint32_t v)
{
    p[0] = (uint8_t)v;
    p[1] = (uint8_t)(v >> 8);
    p[2] = (uint8_t)(v >> 16);
    p[3] = (uint8_t)(v >> 24);
}

static uint32_t bf_copy_bytes(uint8_t *dst, const uint8_t *src, uint32_t n)
{
    uint32_t i;
    for (i = 0; i < n; i++) dst[i] = src[i];
    return n;
}

static char bf_lower(char c)
{
    if (c >= 'A' && c <= 'Z') return (char)(c + ('a' - 'A'));
    return c;
}


static int bf_under(const char *path, const char *root)
{
    uint32_t i = 0;
    while (root[i]) {
        if (bf_lower(path[i]) != bf_lower(root[i])) return 0;
        i++;
    }
    return path[i] == '\0' || path[i] == '/';
}

static int bf_protected(const char *path)
{
    /* Canopus state is enforced by the Supervisor as well; keep our own
     * rejection so the UI gets a deterministic answer. */
    if (bf_under(path, "/data/canopus")) return 1;
    if (bf_under(path, "/canopus")) return 1;
    if (bf_under(path, "/dev")) return 1;
    if (bf_under(path, "/proc")) return 1;
    if (bf_under(path, "/sys")) return 1;
    return 0;
}

static int bf_normalize(const char *in, char *out)
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
            if (o + n + 2u >= CANOPUS_FS_PATH_MAX) return -1;
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

static int bf_parse_path(const uint8_t *p, uint32_t plen, uint32_t *used, char *out)
{
    uint16_t n;
    uint32_t i;
    char raw[CANOPUS_FS_PATH_MAX];
    if (plen < 2u) return -1;
    n = bf_get_u16(p);
    if (n == 0u || n >= CANOPUS_FS_PATH_MAX || plen < (uint32_t)n + 2u) return -1;
    for (i = 0; i < n; i++) {
        if (p[2u + i] == 0u) return -1;
        raw[i] = (char)p[2u + i];
    }
    raw[n] = '\0';
    if (bf_normalize(raw, out) != 0) return -1;
    if (used) *used = (uint32_t)n + 2u;
    return 0;
}

static uint8_t bf_status_from_err(int32_t err)
{
    switch (err) {
    case CANOPUS_SVC_OK: return BF_ST_OK;
    case CANOPUS_SVC_ERR_NOTFOUND: return BF_ST_NOTFOUND;
    case CANOPUS_SVC_ERR_ACCESS: return BF_ST_ACCESS;
    case CANOPUS_SVC_ERR_EXIST: return BF_ST_EXIST;
    case CANOPUS_SVC_ERR_NOTDIR: return BF_ST_NOTDIR;
    case CANOPUS_SVC_ERR_ISDIR: return BF_ST_ISDIR;
    case CANOPUS_SVC_ERR_NOSPACE: return BF_ST_NOSPACE;
    case CANOPUS_SVC_ERR_BUSY:
    case CANOPUS_SVC_ERR_NOTEMPTY: return BF_ST_BUSY;
    case CANOPUS_SVC_ERR_UNSUPPORTED: return BF_ST_UNSUPPORTED;
    case CANOPUS_SVC_ERR_ARGUMENT: return BF_ST_BAD;
    default: return BF_ST_IO;
    }
}

static int bf_write_allowed(const char *path)
{
    if (bf_protected(path)) return 0;
    if (g_dev_mode) return 1;
    return bf_under(path, "/data");
}

static int bf_read_allowed(const char *path)
{
    return !bf_protected(path);
}

static void bf_send(const struct canopus_context_v1 *ctx, uint8_t op, uint16_t req,
                    uint8_t status, const uint8_t *payload, uint32_t plen)
{
    uint8_t frame[BF_MAX_FRAME];
    uint32_t n = 4u + plen;
    if (ctx == 0 || ctx->connect == 0 || ctx->connect->send == 0) return;
    if (plen > BF_PAYLOAD_MAX) return;
    frame[0] = (uint8_t)(op | 0x80u);
    bf_put_u16(&frame[1], req);
    frame[3] = status;
    if (plen) bf_copy_bytes(&frame[4], payload, n - 4u);
    (void)ctx->connect->send(ctx, CANOPUS_CONNECT_BINARY, frame, n);
}

static void bf_event(const struct canopus_context_v1 *ctx, uint8_t code,
                     const uint8_t *payload, uint32_t plen)
{
    uint8_t frame[BF_MAX_FRAME];
    uint32_t n = 1u + plen;
    if (ctx == 0 || ctx->connect == 0 || ctx->connect->send == 0) return;
    if (n > BF_MAX_FRAME) n = BF_MAX_FRAME;
    frame[0] = code;
    if (plen) bf_copy_bytes(&frame[1], payload, n - 1u);
    (void)ctx->connect->send(ctx, CANOPUS_CONNECT_BINARY, frame, n);
}

static void bf_close_read(void)
{
    if (g_read_fd >= 0 && g_ctx != 0 && g_ctx->fs != 0 && g_ctx->fs->close) {
        (void)g_ctx->fs->close(g_ctx, g_read_fd);
    }
    g_read_fd = -1;
    g_read_size = 0;
}

static void bf_discard_write(void)
{
    if (g_write_fd >= 0 && g_ctx != 0 && g_ctx->fs != 0 && g_ctx->fs->close) {
        (void)g_ctx->fs->close(g_ctx, g_write_fd);
    }
    g_write_fd = -1;
    if (g_write_temp[0] != '\0' && g_ctx != 0 && g_ctx->fs != 0 && g_ctx->fs->remove) {
        (void)g_ctx->fs->remove(g_ctx, g_write_temp);
    }
    g_write_temp[0] = '\0';
    g_write_target[0] = '\0';
    g_write_expected = 0;
    g_write_written = 0;
    g_write_crc = 0xFFFFFFFFu;
    g_write_reported = 0;
    g_write_overwrite = 0;
}

static uint32_t bf_crc32_update(uint32_t crc, const uint8_t *data, uint32_t len)
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

static int bf_temp_for(const char *target, char *out)
{
    uint32_t n = (uint32_t)bf_strlen(target);
    uint32_t cut = n;
    uint32_t dir_len;
    if (n == 0 || n + 9u >= CANOPUS_FS_PATH_MAX) return -1;
    while (cut > 0 && target[cut - 1] != '/') cut--;
    dir_len = cut;
    if (dir_len == 0) {
        out[0] = '/';
        bf_copy_bytes((uint8_t *)out + 1, (const uint8_t *)".bftmp", 7);
        out[8] = '\0';
    } else {
        uint32_t i;
        for (i = 0; i < dir_len; i++) out[i] = target[i];
        bf_copy_bytes((uint8_t *)out + dir_len, (const uint8_t *)".bftmp", 7);
        out[dir_len + 7] = '\0';
    }
    return 0;
}

static int bf_path_join(const char *dir, const char *name, char *out)
{
    uint32_t dl = (uint32_t)bf_strlen(dir);
    uint32_t nl = (uint32_t)bf_strlen(name);
    if (dl + nl + 2u >= CANOPUS_FS_PATH_MAX) return -1;
    if (dl == 1u && dir[0] == '/') {
        out[0] = '/';
        bf_copy_bytes((uint8_t *)out + 1, (const uint8_t *)name, nl);
        out[nl + 1] = '\0';
    } else {
        bf_copy_bytes((uint8_t *)out, (const uint8_t *)dir, dl);
        out[dl] = '/';
        bf_copy_bytes((uint8_t *)out + dl + 1, (const uint8_t *)name, nl);
        out[dl + nl + 1] = '\0';
    }
    return 0;
}

static void bf_handle_hello(const struct canopus_context_v1 *ctx, uint16_t req)
{
    uint8_t out[16];
    uint32_t n = 0;
    out[n++] = BF_PROTO_VERSION;
    bf_put_u16(&out[n], (uint16_t)BF_CHUNK); n += 2u;
    out[n++] = (uint8_t)(BF_FLAG_DEV_MODE | BF_FLAG_RMDIR);
    out[n++] = g_dev_mode;
    bf_put_u16(&out[n], (uint16_t)BF_MAX_PATH); n += 2u;
    bf_put_u32(&out[n], 0u); n += 4u;
    bf_send(ctx, BF_OP_HELLO, req, BF_ST_OK, out, n);
}

static void bf_handle_list(const struct canopus_context_v1 *ctx, uint16_t req,
                           const uint8_t *p, uint32_t plen)
{
    char path[CANOPUS_FS_PATH_MAX];
    uint32_t used = 0, offset, idx = 0, count = 0, total = 0;
    uint8_t limit;
    uint8_t out[BF_MAX_FRAME];
    uint32_t o = 0;
    void *dir;
    if (bf_parse_path(p, plen, &used, path) != 0 || plen < used + 5u) {
        bf_send(ctx, BF_OP_LIST, req, BF_ST_BAD, 0, 0);
        return;
    }
    if (!bf_read_allowed(path)) {
        bf_send(ctx, BF_OP_LIST, req, BF_ST_ACCESS, 0, 0);
        return;
    }
    offset = bf_get_u32(p + used);
    limit = p[used + 4u];
    if (limit > BF_MAX_ENTRIES) limit = BF_MAX_ENTRIES;
    if (ctx == 0 || ctx->fs == 0 || ctx->fs->opendir == 0) {
        bf_send(ctx, BF_OP_LIST, req, BF_ST_UNSUPPORTED, 0, 0);
        return;
    }
    dir = ctx->fs->opendir(ctx, path);
    if (dir == 0) {
        bf_send(ctx, BF_OP_LIST, req, BF_ST_NOTFOUND, 0, 0);
        return;
    }
    o = 6u; /* total/offset/count placeholder */
    while (ctx->fs->readdir != 0) {
        struct canopus_fs_dirent ent;
        int32_t rc = ctx->fs->readdir(ctx, dir, &ent);
        char full[CANOPUS_FS_PATH_MAX];
        struct canopus_fs_stat st;
        uint32_t nl;
        uint32_t need;
        if (rc == 1) break;
        if (rc < 0) {
            (void)ctx->fs->closedir(ctx, dir);
            bf_send(ctx, BF_OP_LIST, req, bf_status_from_err(rc), 0, 0);
            return;
        }
        if (ent.name[0] == '.' && (ent.name[1] == '\0' ||
            (ent.name[1] == '.' && ent.name[2] == '\0'))) continue;
        nl = (uint32_t)bf_strlen(ent.name);
        total++;
        if (idx++ < offset) continue;
        if (count >= limit) continue;
        need = 10u + nl;
        if (o + need > BF_PAYLOAD_MAX) continue;
        st.size = 0;
        st.mtime = 0;
        st.is_dir = ent.is_dir;
        if (bf_path_join(path, ent.name, full) == 0 && ctx->fs->stat != 0) {
            struct canopus_fs_stat tmp;
            if (ctx->fs->stat(ctx, full, &tmp) == 0) {
                st.size = tmp.size;
                st.mtime = tmp.mtime;
                st.is_dir = tmp.is_dir;
            }
        }
        out[o++] = (uint8_t)nl;
        bf_copy_bytes(&out[o], (const uint8_t *)ent.name, nl); o += nl;
        out[o++] = (uint8_t)(st.is_dir ? 1u : 0u);
        bf_put_u32(&out[o], (uint32_t)st.size); o += 4u;
        bf_put_u32(&out[o], (uint32_t)st.mtime); o += 4u;
        count++;
    }
    (void)ctx->fs->closedir(ctx, dir);
    bf_put_u16(&out[0], (uint16_t)(total > 0xFFFFu ? 0xFFFFu : total));
    bf_put_u16(&out[2], (uint16_t)(offset > 0xFFFFu ? 0xFFFFu : offset));
    bf_put_u16(&out[4], (uint16_t)count);
    bf_send(ctx, BF_OP_LIST, req, BF_ST_OK, out, o);
}

static void bf_handle_stat(const struct canopus_context_v1 *ctx, uint16_t req,
                           const uint8_t *p, uint32_t plen)
{
    char path[CANOPUS_FS_PATH_MAX];
    struct canopus_fs_stat st;
    uint8_t out[9];
    int32_t rc;
    if (bf_parse_path(p, plen, 0, path) != 0) {
        bf_send(ctx, BF_OP_STAT, req, BF_ST_BAD, 0, 0);
        return;
    }
    if (!bf_read_allowed(path)) {
        bf_send(ctx, BF_OP_STAT, req, BF_ST_ACCESS, 0, 0);
        return;
    }
    if (ctx == 0 || ctx->fs == 0 || ctx->fs->stat == 0) {
        bf_send(ctx, BF_OP_STAT, req, BF_ST_UNSUPPORTED, 0, 0);
        return;
    }
    rc = ctx->fs->stat(ctx, path, &st);
    if (rc < 0) {
        bf_send(ctx, BF_OP_STAT, req, bf_status_from_err(rc), 0, 0);
        return;
    }
    out[0] = (uint8_t)(st.is_dir ? 1u : 0u);
    bf_put_u32(&out[1], (uint32_t)st.size);
    bf_put_u32(&out[5], (uint32_t)st.mtime);
    bf_send(ctx, BF_OP_STAT, req, BF_ST_OK, out, 9);
}

static void bf_handle_mkdir(const struct canopus_context_v1 *ctx, uint16_t req,
                            const uint8_t *p, uint32_t plen)
{
    char path[CANOPUS_FS_PATH_MAX];
    int32_t rc;
    if (bf_parse_path(p, plen, 0, path) != 0) {
        bf_send(ctx, BF_OP_MKDIR, req, BF_ST_BAD, 0, 0);
        return;
    }
    if (!bf_write_allowed(path)) {
        bf_send(ctx, BF_OP_MKDIR, req, BF_ST_ACCESS, 0, 0);
        return;
    }
    rc = ctx->fs->mkdir(ctx, path);
    bf_send(ctx, BF_OP_MKDIR, req, rc < 0 ? bf_status_from_err(rc) : BF_ST_OK, 0, 0);
}

static void bf_handle_remove(const struct canopus_context_v1 *ctx, uint16_t req,
                             const uint8_t *p, uint32_t plen, uint8_t op)
{
    char path[CANOPUS_FS_PATH_MAX];
    int32_t rc;
    if (bf_parse_path(p, plen, 0, path) != 0) {
        bf_send(ctx, op, req, BF_ST_BAD, 0, 0);
        return;
    }
    if (!bf_write_allowed(path)) {
        bf_send(ctx, op, req, BF_ST_ACCESS, 0, 0);
        return;
    }
    if (op == BF_OP_RMDIR) {
        if (!canopus_services_has(ctx, CANOPUS_SVC_FS, 2) || ctx->fs->rmdir == 0) {
            bf_send(ctx, op, req, BF_ST_UNSUPPORTED, 0, 0);
            return;
        }
        rc = ctx->fs->rmdir(ctx, path);
    } else {
        rc = ctx->fs->remove(ctx, path);
    }
    bf_send(ctx, op, req, rc < 0 ? bf_status_from_err(rc) : BF_ST_OK, 0, 0);
}

static void bf_handle_rename(const struct canopus_context_v1 *ctx, uint16_t req,
                             const uint8_t *p, uint32_t plen)
{
    char from[CANOPUS_FS_PATH_MAX];
    char to[CANOPUS_FS_PATH_MAX];
    uint32_t used = 0;
    int32_t rc;
    if (bf_parse_path(p, plen, &used, from) != 0 || used >= plen ||
        bf_parse_path(p + used, plen - used, 0, to) != 0) {
        bf_send(ctx, BF_OP_RENAME, req, BF_ST_BAD, 0, 0);
        return;
    }
    if (!bf_write_allowed(from) || !bf_write_allowed(to)) {
        bf_send(ctx, BF_OP_RENAME, req, BF_ST_ACCESS, 0, 0);
        return;
    }
    rc = ctx->fs->rename(ctx, from, to);
    bf_send(ctx, BF_OP_RENAME, req, rc < 0 ? bf_status_from_err(rc) : BF_ST_OK, 0, 0);
}

static void bf_handle_copy(const struct canopus_context_v1 *ctx, uint16_t req,
                          const uint8_t *p, uint32_t plen)
{
    char from[CANOPUS_FS_PATH_MAX];
    char to[CANOPUS_FS_PATH_MAX];
    char temp[CANOPUS_FS_PATH_MAX];
    struct canopus_fs_stat st;
    uint32_t first = 0, second = 0;
    uint8_t overwrite;
    int32_t src = -1, dst = -1, rc;
    uint8_t buf[512];
    if (bf_parse_path(p, plen, &first, from) != 0 ||
        bf_parse_path(p + first, plen - first, &second, to) != 0 ||
        plen < first + second + 1u) {
        bf_send(ctx, BF_OP_COPY, req, BF_ST_BAD, 0, 0);
        return;
    }
    overwrite = p[first + second];
    if (!bf_read_allowed(from) || !bf_write_allowed(to)) {
        bf_send(ctx, BF_OP_COPY, req, BF_ST_ACCESS, 0, 0);
        return;
    }
    if (bf_strcmp(from, to)) {
        /* Different paths, continue. */
    } else {
        bf_send(ctx, BF_OP_COPY, req, BF_ST_BAD, 0, 0);
        return;
    }
    if (ctx->fs->stat(ctx, from, &st) < 0) {
        bf_send(ctx, BF_OP_COPY, req, BF_ST_NOTFOUND, 0, 0);
        return;
    }
    if (st.is_dir) {
        bf_send(ctx, BF_OP_COPY, req, BF_ST_ISDIR, 0, 0);
        return;
    }
    if (ctx->fs->stat(ctx, to, &st) == 0) {
        if (st.is_dir) {
            bf_send(ctx, BF_OP_COPY, req, BF_ST_ISDIR, 0, 0);
            return;
        }
        if (!overwrite) {
            bf_send(ctx, BF_OP_COPY, req, BF_ST_EXIST, 0, 0);
            return;
        }
    }
    if (bf_temp_for(to, temp) != 0) {
        bf_send(ctx, BF_OP_COPY, req, BF_ST_BAD, 0, 0);
        return;
    }
    (void)ctx->fs->remove(ctx, temp);
    src = ctx->fs->open(ctx, from, CANOPUS_FS_READ);
    if (src < 0) {
        bf_send(ctx, BF_OP_COPY, req, bf_status_from_err(src), 0, 0);
        return;
    }
    dst = ctx->fs->open(ctx, temp, CANOPUS_FS_WRITE | CANOPUS_FS_CREATE | CANOPUS_FS_TRUNC);
    if (dst < 0) {
        (void)ctx->fs->close(ctx, src);
        bf_send(ctx, BF_OP_COPY, req, bf_status_from_err(dst), 0, 0);
        return;
    }
    for (;;) {
        int32_t got = ctx->fs->read(ctx, src, buf, sizeof(buf));
        uint32_t sent = 0;
        if (got < 0) {
            rc = got;
            goto fail;
        }
        if (got == 0) break;
        while (sent < (uint32_t)got) {
            int32_t wrote = ctx->fs->write(ctx, dst, buf + sent, (uint32_t)got - sent);
            if (wrote <= 0) {
                rc = wrote < 0 ? wrote : CANOPUS_SVC_ERR_IO;
                goto fail;
            }
            sent += (uint32_t)wrote;
        }
    }
    if (ctx->fs->sync != 0) (void)ctx->fs->sync(ctx, dst);
    rc = ctx->fs->close(ctx, dst);
    dst = -1;
    if (rc < 0) goto fail;
    (void)ctx->fs->close(ctx, src);
    src = -1;
    rc = ctx->fs->rename(ctx, temp, to);
    if (rc == CANOPUS_SVC_ERR_EXIST && overwrite) {
        (void)ctx->fs->remove(ctx, to);
        rc = ctx->fs->rename(ctx, temp, to);
    }
    if (rc < 0) {
        (void)ctx->fs->remove(ctx, temp);
        bf_send(ctx, BF_OP_COPY, req, bf_status_from_err(rc), 0, 0);
        return;
    }
    bf_send(ctx, BF_OP_COPY, req, BF_ST_OK, 0, 0);
    return;
fail:
    if (dst >= 0) (void)ctx->fs->close(ctx, dst);
    if (src >= 0) (void)ctx->fs->close(ctx, src);
    (void)ctx->fs->remove(ctx, temp);
    bf_send(ctx, BF_OP_COPY, req, bf_status_from_err(rc), 0, 0);
}

static void bf_handle_open_read(const struct canopus_context_v1 *ctx, uint16_t req,
                                const uint8_t *p, uint32_t plen)
{
    char path[CANOPUS_FS_PATH_MAX];
    uint8_t out[4];
    int32_t fd;
    int64_t size;
    if (bf_parse_path(p, plen, 0, path) != 0) {
        bf_send(ctx, BF_OP_OPEN_READ, req, BF_ST_BAD, 0, 0);
        return;
    }
    if (!bf_read_allowed(path)) {
        bf_send(ctx, BF_OP_OPEN_READ, req, BF_ST_ACCESS, 0, 0);
        return;
    }
    bf_close_read();
    fd = ctx->fs->open(ctx, path, CANOPUS_FS_READ);
    if (fd < 0) {
        bf_send(ctx, BF_OP_OPEN_READ, req, bf_status_from_err(fd), 0, 0);
        return;
    }
    size = ctx->fs->size(ctx, fd);
    if (size < 0) {
        (void)ctx->fs->close(ctx, fd);
        bf_send(ctx, BF_OP_OPEN_READ, req, bf_status_from_err((int32_t)size), 0, 0);
        return;
    }
    g_read_fd = fd;
    g_read_size = (uint32_t)size;
    bf_put_u32(out, g_read_size);
    bf_send(ctx, BF_OP_OPEN_READ, req, BF_ST_OK, out, 4);
}

static void bf_handle_read(const struct canopus_context_v1 *ctx, uint16_t req,
                           const uint8_t *p, uint32_t plen)
{
    uint8_t out[BF_CHUNK];
    uint32_t offset, want;
    int32_t rc;
    if (g_read_fd < 0 || plen < 6u) {
        bf_send(ctx, BF_OP_READ, req, BF_ST_BAD, 0, 0);
        return;
    }
    offset = bf_get_u32(p);
    want = bf_get_u16(p + 4);
    if (want > BF_CHUNK) want = BF_CHUNK;
    if (offset >= g_read_size) {
        bf_send(ctx, BF_OP_READ, req, BF_ST_OK, 0, 0);
        return;
    }
    if (want > g_read_size - offset) want = g_read_size - offset;
    if (ctx->fs->seek(ctx, g_read_fd, (int64_t)offset, CANOPUS_FS_SEEK_SET) < 0) {
        bf_send(ctx, BF_OP_READ, req, BF_ST_IO, 0, 0);
        return;
    }
    {
        uint32_t got = 0;
        while (got < want) {
            rc = ctx->fs->read(ctx, g_read_fd, out + got, want - got);
            if (rc < 0) {
                bf_send(ctx, BF_OP_READ, req, bf_status_from_err(rc), 0, 0);
                return;
            }
            if (rc == 0) break;
            got += (uint32_t)rc;
        }
        bf_send(ctx, BF_OP_READ, req, BF_ST_OK, out, got);
    }
}

static void bf_handle_open_write(const struct canopus_context_v1 *ctx, uint16_t req,
                                 const uint8_t *p, uint32_t plen)
{
    char path[CANOPUS_FS_PATH_MAX];
    char temp[CANOPUS_FS_PATH_MAX];
    uint32_t used = 0, expected;
    uint8_t overwrite;
    struct canopus_fs_stat st;
    int32_t fd;
    if (bf_parse_path(p, plen, &used, path) != 0 || plen < used + 5u) {
        bf_send(ctx, BF_OP_OPEN_WRITE, req, BF_ST_BAD, 0, 0);
        return;
    }
    expected = bf_get_u32(p + used);
    overwrite = p[used + 4u];
    if (!bf_write_allowed(path)) {
        bf_send(ctx, BF_OP_OPEN_WRITE, req, BF_ST_ACCESS, 0, 0);
        return;
    }
    if (ctx->fs->stat(ctx, path, &st) == 0 && st.is_dir) {
        bf_send(ctx, BF_OP_OPEN_WRITE, req, BF_ST_ISDIR, 0, 0);
        return;
    }
    if (!overwrite && ctx->fs->stat(ctx, path, &st) == 0) {
        bf_send(ctx, BF_OP_OPEN_WRITE, req, BF_ST_EXIST, 0, 0);
        return;
    }
    if (bf_temp_for(path, temp) != 0) {
        bf_send(ctx, BF_OP_OPEN_WRITE, req, BF_ST_BAD, 0, 0);
        return;
    }
    bf_discard_write();
    (void)ctx->fs->remove(ctx, temp);
    fd = ctx->fs->open(ctx, temp, CANOPUS_FS_WRITE | CANOPUS_FS_CREATE | CANOPUS_FS_TRUNC);
    if (fd < 0) {
        bf_send(ctx, BF_OP_OPEN_WRITE, req, bf_status_from_err(fd), 0, 0);
        return;
    }
    g_write_fd = fd;
    bf_copy_bytes((uint8_t *)g_write_target, (const uint8_t *)path, (uint32_t)bf_strlen(path) + 1u);
    bf_copy_bytes((uint8_t *)g_write_temp, (const uint8_t *)temp, (uint32_t)bf_strlen(temp) + 1u);
    g_write_expected = expected;
    g_write_written = 0;
    g_write_reported = 0;
    g_write_crc = 0xFFFFFFFFu;
    g_write_overwrite = overwrite;
    bf_send(ctx, BF_OP_OPEN_WRITE, req, BF_ST_OK, 0, 0);
}

static void bf_handle_write(const struct canopus_context_v1 *ctx, uint16_t req,
                            const uint8_t *p, uint32_t plen)
{
    uint32_t offset, n;
    int32_t rc;
    if (g_write_fd < 0 || plen < 4u) {
        bf_send(ctx, BF_OP_WRITE, req, BF_ST_BAD, 0, 0);
        return;
    }
    offset = bf_get_u32(p);
    n = plen - 4u;
    if (offset != g_write_written) {
        bf_send(ctx, BF_OP_WRITE, req, BF_ST_SEQUENCE, 0, 0);
        return;
    }
    if (g_write_written > g_write_expected ||
        n > g_write_expected - g_write_written) {
        bf_send(ctx, BF_OP_WRITE, req, BF_ST_TOO_LARGE, 0, 0);
        return;
    }
    {
        uint32_t sent = 0;
        while (sent < n) {
            rc = ctx->fs->write(ctx, g_write_fd, p + 4 + sent, n - sent);
            if (rc <= 0) {
                bf_send(ctx, BF_OP_WRITE, req, rc < 0 ? bf_status_from_err(rc) : BF_ST_IO, 0, 0);
                return;
            }
            sent += (uint32_t)rc;
        }
    }
    g_write_crc = bf_crc32_update(g_write_crc, p + 4, n);
    g_write_written += n;
    if (g_write_written - g_write_reported >= BF_REPORT_STEP ||
        g_write_written == g_write_expected) {
        uint8_t ev[8];
        bf_put_u32(&ev[0], g_write_written);
        bf_put_u32(&ev[4], g_write_crc ^ 0xFFFFFFFFu);
        bf_event(ctx, 0xC1u, ev, 8u);
        g_write_reported = g_write_written;
    }
    bf_send(ctx, BF_OP_WRITE, req, BF_ST_OK, 0, 0);
}

static void bf_handle_commit(const struct canopus_context_v1 *ctx, uint16_t req,
                             const uint8_t *p, uint32_t plen)
{
    uint32_t expected_crc;
    int32_t rc;
    if (g_write_fd < 0 || plen < 4u) {
        bf_send(ctx, BF_OP_COMMIT, req, BF_ST_BAD, 0, 0);
        return;
    }
    expected_crc = bf_get_u32(p);
    if (g_write_written != g_write_expected) {
        bf_send(ctx, BF_OP_COMMIT, req, BF_ST_IO, 0, 0);
        return;
    }
    if ((g_write_crc ^ 0xFFFFFFFFu) != expected_crc) {
        bf_discard_write();
        bf_send(ctx, BF_OP_COMMIT, req, BF_ST_CRC, 0, 0);
        return;
    }
    if (ctx->fs->sync != 0) (void)ctx->fs->sync(ctx, g_write_fd);
    rc = ctx->fs->close(ctx, g_write_fd);
    g_write_fd = -1;
    if (rc < 0) {
        bf_discard_write();
        bf_send(ctx, BF_OP_COMMIT, req, bf_status_from_err(rc), 0, 0);
        return;
    }
    rc = ctx->fs->rename(ctx, g_write_temp, g_write_target);
    if (rc == CANOPUS_SVC_ERR_EXIST && g_write_overwrite) {
        (void)ctx->fs->remove(ctx, g_write_target);
        rc = ctx->fs->rename(ctx, g_write_temp, g_write_target);
    }
    if (rc < 0) {
        bf_discard_write();
        bf_send(ctx, BF_OP_COMMIT, req, bf_status_from_err(rc), 0, 0);
        return;
    }
    g_write_temp[0] = '\0';
    g_write_target[0] = '\0';
    g_write_expected = 0;
    g_write_written = 0;
    g_write_crc = 0xFFFFFFFFu;
    g_write_reported = 0;
    g_write_overwrite = 0;
    bf_send(ctx, BF_OP_COMMIT, req, BF_ST_OK, 0, 0);
}

static void bf_handle_set_dev(const struct canopus_context_v1 *ctx, uint16_t req,
                              const uint8_t *p, uint32_t plen)
{
    if (plen < 1u) {
        bf_send(ctx, BF_OP_SET_DEV, req, BF_ST_BAD, 0, 0);
        return;
    }
    if (ctx->kv == 0 || ctx->kv->set_bool == 0 ||
        !canopus_services_has(ctx, CANOPUS_SVC_KV, 1)) {
        bf_send(ctx, BF_OP_SET_DEV, req, BF_ST_UNSUPPORTED, 0, 0);
        return;
    }
    if (ctx->kv->set_bool(ctx, "dev_mode", p[0] ? 1 : 0) != 0) {
        bf_send(ctx, BF_OP_SET_DEV, req, BF_ST_IO, 0, 0);
        return;
    }
    g_dev_mode = p[0] ? 1u : 0u;
    bf_send(ctx, BF_OP_SET_DEV, req, BF_ST_OK, 0, 0);
}

static void bf_dispatch(const struct canopus_context_v1 *ctx, const uint8_t *data, uint32_t len)
{
    uint8_t op;
    uint16_t req;
    const uint8_t *p;
    uint32_t plen;
    if (len < 3u) return;
    op = data[0];
    req = bf_get_u16(data + 1);
    p = data + 3;
    plen = len - 3u;
    if (ctx == 0 || ctx->fs == 0) {
        bf_send(ctx, op, req, BF_ST_UNSUPPORTED, 0, 0);
        return;
    }
    switch (op) {
    case BF_OP_HELLO: bf_handle_hello(ctx, req); break;
    case BF_OP_LIST: bf_handle_list(ctx, req, p, plen); break;
    case BF_OP_STAT: bf_handle_stat(ctx, req, p, plen); break;
    case BF_OP_MKDIR: bf_handle_mkdir(ctx, req, p, plen); break;
    case BF_OP_REMOVE: bf_handle_remove(ctx, req, p, plen, op); break;
    case BF_OP_RMDIR: bf_handle_remove(ctx, req, p, plen, op); break;
    case BF_OP_RENAME: bf_handle_rename(ctx, req, p, plen); break;
    case BF_OP_COPY: bf_handle_copy(ctx, req, p, plen); break;
    case BF_OP_OPEN_READ: bf_handle_open_read(ctx, req, p, plen); break;
    case BF_OP_READ: bf_handle_read(ctx, req, p, plen); break;
    case BF_OP_CLOSE_READ:
        bf_close_read();
        bf_send(ctx, op, req, BF_ST_OK, 0, 0);
        break;
    case BF_OP_OPEN_WRITE: bf_handle_open_write(ctx, req, p, plen); break;
    case BF_OP_WRITE: bf_handle_write(ctx, req, p, plen); break;
    case BF_OP_COMMIT: bf_handle_commit(ctx, req, p, plen); break;
    case BF_OP_ABORT:
        bf_discard_write();
        bf_send(ctx, op, req, BF_ST_OK, 0, 0);
        break;
    case BF_OP_PING: {
        uint8_t one = 1u;
        bf_send(ctx, op, req, BF_ST_OK, &one, 1u);
        break;
    }
    case BF_OP_SET_DEV: bf_handle_set_dev(ctx, req, p, plen); break;
    case BF_OP_GET_DEV: {
        uint8_t one = g_dev_mode;
        bf_send(ctx, op, req, BF_ST_OK, &one, 1u);
        break;
    }
    default:
        bf_send(ctx, op, req, BF_ST_BAD, 0, 0);
        break;
    }
}

static void bf_on_connect(const struct canopus_context_v1 *ctx, uint32_t kind,
                          const void *data, uint32_t len, void *user_data)
{
    (void)user_data;
    if (kind != CANOPUS_CONNECT_BINARY || data == 0 || len == 0u) return;
    bf_dispatch(ctx, (const uint8_t *)data, len);
}

static int32_t bf_prepare(const struct canopus_context_v1 *ctx)
{
    g_ctx = ctx;
    g_dev_mode = 0;
    g_read_fd = -1;
    g_read_size = 0;
    g_write_fd = -1;
    g_write_expected = 0;
    g_write_written = 0;
    g_write_crc = 0xFFFFFFFFu;
    g_write_reported = 0;
    g_write_overwrite = 0;
    g_write_temp[0] = '\0';
    g_write_target[0] = '\0';
    return 0;
}

static int32_t bf_activate(const struct canopus_context_v1 *ctx)
{
    g_ctx = ctx;
    g_dev_mode = 0;
    /* Keep activation minimal. In particular do not touch the filesystem or
     * the KV store here: the manager may load a module from inside a
     * /dev/canopus command, and a nested file open is not worth the risk.
     * Dev mode is read on the first HELLO instead. */
    if (ctx == 0 || ctx->connect == 0 || ctx->connect->listen == 0 ||
        !canopus_services_has(ctx, CANOPUS_SVC_CONNECT, 1)) return 0;
    (void)ctx->connect->listen(ctx, bf_on_connect, 0);
    return 0;
}

static int32_t bf_deactivate(const struct canopus_context_v1 *ctx)
{
    bf_close_read();
    bf_discard_write();
    if (ctx != 0 && ctx->connect != 0 && ctx->connect->listen != 0) {
        (void)ctx->connect->listen(ctx, 0, 0);
    }
    return 0;
}

static int32_t bf_stop(const struct canopus_context_v1 *ctx)
{
    (void)bf_deactivate(ctx);
    g_ctx = 0;
    return 0;
}

static int32_t bf_query(struct canopus_status_writer_v1 *writer)
{
    (void)writer;
    return 0;
}

#ifndef BF_BRIDGE
static const struct canopus_module_descriptor_v1 g_descriptor = {
    .struct_size = sizeof(struct canopus_module_descriptor_v1),
    .abi_major = CANOPUS_ABI_MAJOR,
    .abi_minor = CANOPUS_ABI_MINOR,
    .flags = 0u,
    .module_id = "band_files",
    .module_version = "0.1.6",
    .build_id = "band_files-0.1.6",
    .target_id = CANOPUS_TARGET_ID,
    .prepare = bf_prepare,
    .activate = bf_activate,
    .deactivate = bf_deactivate,
    .stop = bf_stop,
    .query = bf_query,
};
#endif /* !BF_BRIDGE */

#if !defined(BF_TEST) && !defined(BF_BRIDGE)
/* GCC 15.2 at -Os was observed to keep the CMR1 frame construction but drop
 * the open/write/close calls. Registration is the module's only side effect
 * at load time, so force a real out-of-line call at -O0. */
static int bf_register_module(const struct canopus_context_v1 *ctx)
    __attribute__((noinline, optimize("O0")));
static int bf_register_module(const struct canopus_context_v1 *ctx)
{
    return canopus_module_register(ctx, &g_descriptor);
}

__attribute__((constructor)) static void bf_module_entry(
    const struct canopus_context_v1 *ctx)
{
    (void)bf_register_module(ctx);
}
#endif

#ifdef BF_BRIDGE
void bf_embed_dispatch(const struct canopus_context_v1 *ctx,
                       const uint8_t *data, uint32_t len)
{
    bf_dispatch(ctx, data, len);
}
#endif
