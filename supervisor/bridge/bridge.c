/*
 * Chaos native file bridge for AstroBox's existing CanopusConnect plugin.
 *
 * This file deliberately speaks only the CanopusConnect subset of CPC2:
 * ConnectListen, ConnectSend and ConnectMessage.  It does not contain a
 * module registry, a descriptor, a manager or a package installer.
 *
 * The file protocol is implemented by band_files_embed.c, the module source
 * adapted to run on a small in-process fake Canopus context whose fs/kv
 * services call the proven Chaos NuttX wrappers below.
 */
#include <stddef.h>
#include <stdint.h>

#include "canopus_abi.h"
#include "canopus_memory.h"
#include "canopus_protocol.h"
#include "canopus_services.h"

#define BRIDGE_ID           "band_files"
#define BRIDGE_ID_LEN       10u
#define BRIDGE_L2_CHANNEL   0x43u
#define BRIDGE_L2_WRITE_ENC 2u
#define BRIDGE_FRAME_MAX    (CANOPUS_TRANSPORT_V2_HEADER_SIZE + CANOPUS_PROTO_MAX_PAYLOAD)
#define BRIDGE_RX_SLOTS     4u
#define BRIDGE_TX_SLOTS     8u
#define BRIDGE_DEV_MODE_KEY "dev_mode"

/* .043 firmware entry points, all already used by Chaos or by the Canopus
 * target pack. */
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

/* L2 backend addresses from targets/xiaomi-band-10-pro-3.101.043. */
#define L2_RX                 ((uintptr_t)0x0C7D6E6Du)
#define L2_SEND               ((uintptr_t)0x0C7D719Du)
#define L2_SESSION            ((uintptr_t)0x0C7D7E95u)
#define L2_CRYPTO             ((uintptr_t)0x0C7D46A5u)
#define L2_CONNECTIONS        ((uintptr_t)0x200E16B8u)
#define L2_TIMER              ((uintptr_t)0x0C587ED1u)
#define L2_TIMER_DELETE       ((uintptr_t)0x0C588129u)
#define L2_OWNER_LOOP         ((uintptr_t)0x20125700u)
#define L2_PROBE_BT_LOOP      ((uintptr_t)0x201259E4u)
#define L2_OWNER_TIMER_INIT   ((uintptr_t)0x0C2E3995u)
#define L2_OWNER_TIMER_START  ((uintptr_t)0x0C2E3B85u)
#define L2_OWNER_TIMER_SIZE   136u

#define NUTTX_O_RDONLY 1u
#define NUTTX_O_WRONLY 2u
#define NUTTX_O_RDWR   3u
#define NUTTX_O_CREAT  4u
#define NUTTX_SEEK_END 2u

struct l2_packet {
    uint8_t channel;
    uint8_t opcode;
    uint16_t reserved;
    void *payload;
    uint16_t len;
    uint16_t padding;
    void *complete;
    void *complete_cookie;
    void *sent;
    void *sent_cookie;
};

typedef void (*l2_rx_fn)(uint16_t, uint8_t, uint8_t, void *, uint16_t);
typedef void *(*session_fn)(uint16_t);
typedef int (*crypto_fn)(void *, int, uint16_t, const void *, void *);
typedef int (*send_fn)(void *, const struct l2_packet *);
typedef int (*timer_init_fn)(void *, void *);
typedef int (*timer_start_fn)(void *, void (*)(void *), uint64_t, uint64_t);
typedef void (*timer_delete_fn)(void *);
typedef void *(*timer_fn)(void (*)(void *), uint32_t, void *);
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

struct rx_req {
    volatile uint32_t state;
    uint16_t handle;
    uint16_t len;
    void *session;
    uint8_t data[BRIDGE_FRAME_MAX];
};

struct tx_req {
    volatile uint32_t state;
    uint16_t handle;
    uint16_t len;
    void *session;
    uint8_t data[BRIDGE_FRAME_MAX];
};

static struct rx_req g_rx[BRIDGE_RX_SLOTS];
static struct tx_req g_tx[BRIDGE_TX_SLOTS];
static uint32_t g_rx_read, g_rx_write;
static uint32_t g_tx_read, g_tx_write;
static uint16_t g_last_handle;
static void *g_last_session;
static int g_connected;
static int g_listener;
static int g_owner_init;
static uint64_t g_owner_timer[(L2_OWNER_TIMER_SIZE + 7u) / 8u];
static void *g_boot_timer;
static int g_started;
static int g_logged_hook, g_logged_rx, g_logged_crypto, g_logged_send;
static int g_done_hook, g_done_rx, g_done_crypto, g_done_send;
static int g_bt_init;
static uint32_t g_bt_period;
static uint64_t g_bt_timer[(L2_OWNER_TIMER_SIZE + 7u) / 8u];

extern void chaos_bridge_log(const char *message);

static const struct canopus_connect_obj g_connect_obj;
static const struct canopus_fs_obj g_fs_obj;
static const struct canopus_kv_obj g_kv_obj;
static struct canopus_services_v1 g_services;
static struct canopus_context_v1 g_ctx;

void bf_embed_dispatch(const struct canopus_context_v1 *ctx,
                       const uint8_t *data, uint32_t len);

/* ------------------------------------------------------------------ */
/* small helpers                                                       */
/* ------------------------------------------------------------------ */

static uint32_t irq_save(void)
{
    uint32_t mask;
    __asm__ volatile("mrs %0, primask\n\tcpsid i" : "=r"(mask) :: "memory");
    return mask;
}

static void irq_restore(uint32_t mask)
{
    __asm__ volatile("msr primask, %0" :: "r"(mask) : "memory");
}

static int neg_errno(void)
{
    errno_fn get = (errno_fn)FW_ERRNO;
    int *e = get();
    return (e != 0 && *e > 0) ? -*e : CANOPUS_SVC_ERR_IO;
}

static int path_bad(const char *p)
{
    if (p == 0 || p[0] != '/') return 1;
    while (*p != '\0') {
        unsigned char c = (unsigned char)*p++;
        if (c < 0x20u) return 1;
    }
    return 0;
}

/* ------------------------------------------------------------------ */
/* fake Canopus FS service                                            */
/* ------------------------------------------------------------------ */

static int32_t f_open(const struct canopus_context_v1 *ctx, const char *path, uint32_t flags)
{
    (void)ctx;
    if (path_bad(path)) return CANOPUS_SVC_ERR_ARGUMENT;
    int access;
    int create = 0;
    if ((flags & CANOPUS_FS_WRITE) == 0u) {
        access = NUTTX_O_RDONLY;
    } else {
        access = (flags & CANOPUS_FS_READ) != 0u ? NUTTX_O_RDWR : NUTTX_O_WRONLY;
        create = NUTTX_O_CREAT;
        if ((flags & CANOPUS_FS_APPEND) == 0u) {
            (void)((remove_fn)FW_REMOVE)(path);
        }
    }
    int fd = ((open_fn)FW_OPEN)(path, (int)(access | create), 0666u);
    return fd < 0 ? neg_errno() : fd;
}

static int32_t f_read(const struct canopus_context_v1 *ctx, int32_t fd, void *buf, uint32_t size)
{
    (void)ctx;
    int32_t n = ((read_fn)FW_READ)(fd, buf, size);
    return n < 0 ? neg_errno() : n;
}

static int32_t f_write(const struct canopus_context_v1 *ctx, int32_t fd, const void *buf, uint32_t size)
{
    (void)ctx;
    int32_t n = ((write_fn)FW_WRITE)(fd, buf, size);
    return n < 0 ? neg_errno() : n;
}

static int64_t f_seek(const struct canopus_context_v1 *ctx, int32_t fd, int64_t off, int32_t whence)
{
    (void)ctx;
    int64_t n = ((lseek_fn)FW_LSEEK)(fd, off, (int)whence);
    return n < 0 ? neg_errno() : n;
}

static int64_t f_tell(const struct canopus_context_v1 *ctx, int32_t fd)
{
    return f_seek(ctx, fd, 0, 1);
}

static int64_t f_size(const struct canopus_context_v1 *ctx, int32_t fd)
{
    int64_t end = f_seek(ctx, fd, 0, NUTTX_SEEK_END);
    if (end < 0) return end;
    (void)f_seek(ctx, fd, 0, 0);
    return end;
}

static int32_t f_close(const struct canopus_context_v1 *ctx, int32_t fd)
{
    (void)ctx;
    return ((close_fn)FW_CLOSE)(fd) < 0 ? neg_errno() : 0;
}

static int32_t f_remove(const struct canopus_context_v1 *ctx, const char *path)
{
    (void)ctx;
    return ((remove_fn)FW_REMOVE)(path) < 0 ? neg_errno() : 0;
}

static int32_t f_rename(const struct canopus_context_v1 *ctx, const char *a, const char *b)
{
    (void)ctx;
    return ((rename_fn)FW_RENAME)(a, b) < 0 ? neg_errno() : 0;
}

static int32_t f_stat(const struct canopus_context_v1 *ctx, const char *path,
                      struct canopus_fs_stat *out)
{
    (void)ctx;
    uint32_t st[26];
    if (out == 0) return CANOPUS_SVC_ERR_ARGUMENT;
    for (uint32_t i = 0; i < 26u; i++) st[i] = 0;
    if (((stat_fn)FW_STAT)(path, st) < 0) return neg_errno();
    out->is_dir = ((st[2] & 0xF000u) == 0x4000u) ? 1u : 0u;
    out->size = (uint32_t)st[8];
    out->mtime = 0;
    return 0;
}

static int32_t f_exists(const struct canopus_context_v1 *ctx, const char *path)
{
    struct canopus_fs_stat st;
    return f_stat(ctx, path, &st) == 0 ? 1 : 0;
}

static int32_t f_mkdir(const struct canopus_context_v1 *ctx, const char *path)
{
    (void)ctx;
    return ((mkdir_fn)FW_MKDIR)(path, 0777u) < 0 ? neg_errno() : 0;
}

static void *f_opendir(const struct canopus_context_v1 *ctx, const char *path)
{
    (void)ctx;
    void *d = ((opendir_fn)FW_OPENDIR)(path);
    return d;
}

static int32_t f_readdir(const struct canopus_context_v1 *ctx, void *dir,
                         struct canopus_fs_dirent *out)
{
    (void)ctx;
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

static int32_t f_closedir(const struct canopus_context_v1 *ctx, void *dir)
{
    (void)ctx;
    return ((closedir_fn)FW_CLOSEDIR)(dir) < 0 ? neg_errno() : 0;
}

static int32_t f_rmdir(const struct canopus_context_v1 *ctx, const char *path)
{
    return f_remove(ctx, path);
}

static const struct canopus_fs_obj g_fs_obj = {
    .open = f_open,
    .read = f_read,
    .write = f_write,
    .seek = f_seek,
    .tell = f_tell,
    .size = f_size,
    .sync = 0,
    .close = f_close,
    .remove = f_remove,
    .rename = f_rename,
    .stat = f_stat,
    .exists = f_exists,
    .mkdir = f_mkdir,
    .opendir = f_opendir,
    .readdir = f_readdir,
    .closedir = f_closedir,
    .read_all = 0,
    .write_all = 0,
    .get_data_path = 0,
    .rmdir = f_rmdir,
};

/* ------------------------------------------------------------------ */
/* fake KV service (dev mode is intentionally per-boot)                */
/* ------------------------------------------------------------------ */

static int g_dev_mode;
static int k_get_bool(const struct canopus_context_v1 *ctx, const char *key, int def)
{
    (void)ctx; (void)key;
    return g_dev_mode ? 1 : def;
}
static int32_t k_set_bool(const struct canopus_context_v1 *ctx, const char *key, int val)
{
    (void)ctx; (void)key;
    g_dev_mode = val ? 1 : 0;
    return 0;
}
static const struct canopus_kv_obj g_kv_obj = {
    .get_bool = k_get_bool,
    .set_bool = k_set_bool,
};

/* ------------------------------------------------------------------ */
/* L2 transport                                                       */
/* ------------------------------------------------------------------ */

static void l2_original_rx(uint16_t h, uint8_t ch, uint8_t op, void *p, uint16_t n)
{
    ((l2_rx_fn)L2_RX)(h, ch, op, p, n);
}

static void l2_rx(uint16_t handle, uint8_t channel, uint8_t opcode,
                  void *payload, uint16_t len)
{
    if (channel != BRIDGE_L2_CHANNEL) {
        l2_original_rx(handle, channel, opcode, payload, len);
        return;
    }
    if (opcode != BRIDGE_L2_WRITE_ENC || payload == 0 || len < CANOPUS_TRANSPORT_V2_HEADER_SIZE) {
        return;
    }
    void *session = ((session_fn)L2_SESSION)(handle);
    if (session == 0 || len > BRIDGE_FRAME_MAX) return;
    uint32_t mask = irq_save();
    uint32_t slot = g_rx_write;
    if (g_rx[slot].state != 0u) {
        irq_restore(mask);
        return;
    }
    g_rx[slot].state = 1u;
    g_rx_write = (slot + 1u) % BRIDGE_RX_SLOTS;
    irq_restore(mask);

    int ok = ((crypto_fn)L2_CRYPTO)(session, 0, len, payload, g_rx[slot].data) == 0;
    if (!ok && !g_done_crypto) { g_done_crypto = 1; g_logged_crypto = 1; }
    if (ok && !g_done_rx) { g_done_rx = 1; g_logged_rx = 1; }
    g_rx[slot].len = ok ? len : 0u;
    g_rx[slot].handle = handle;
    g_rx[slot].session = session;
    __asm__ volatile("dmb" ::: "memory");
    g_rx[slot].state = 2u;
}

static void l2_attach(void)
{
    volatile uint32_t *head = (volatile uint32_t *)L2_CONNECTIONS;
    uint32_t mask = irq_save();
    for (volatile uint32_t *node = (volatile uint32_t *)head[1];
         node != 0 && node != head;
         node = (volatile uint32_t *)node[1]) {
        volatile uint32_t *conn = (volatile uint32_t *)node[3];
        if (conn != 0 && conn[3] == (uint32_t)L2_RX) {
            conn[3] = (uint32_t)(uintptr_t)l2_rx;
            if (!g_done_hook) { g_done_hook = 1; g_logged_hook = 1; }
        }
    }
    irq_restore(mask);
}

/// Restore every connection whose RX slot still points at us. If Canopus (or
/// another owner) already replaced it, leave that handler alone.
static void l2_detach(void)
{
    volatile uint32_t *head = (volatile uint32_t *)L2_CONNECTIONS;
    uint32_t mask = irq_save();
    for (volatile uint32_t *node = (volatile uint32_t *)head[1];
         node != 0 && node != head;
         node = (volatile uint32_t *)node[1]) {
        volatile uint32_t *conn = (volatile uint32_t *)node[3];
        if (conn != 0 && conn[3] == (uint32_t)(uintptr_t)l2_rx) {
            conn[3] = (uint32_t)L2_RX;
        }
    }
    irq_restore(mask);
}

static int l2_send_now(uint16_t handle, void *session, const uint8_t *data, uint16_t len)
{
    struct l2_packet pkt;
    uint8_t copy[BRIDGE_FRAME_MAX];
    if (session == 0 || len == 0u || len > sizeof(copy)) return -1;
    canopus_memcpy(copy, data, len);
    for (uint32_t i = 0; i < sizeof(pkt); i++) ((uint8_t *)&pkt)[i] = 0;
    pkt.channel = BRIDGE_L2_CHANNEL;
    pkt.opcode = BRIDGE_L2_WRITE_ENC;
    pkt.payload = copy;
    pkt.len = len;
    (void)handle;
    int rc = ((send_fn)L2_SEND)(session, &pkt);
    if (rc != 0 && !g_done_send) { g_done_send = 1; g_logged_send = 1; }
    return rc == 0 ? 0 : -1;
}

static int tx_enqueue(const uint8_t *data, uint16_t len)
{
    if (len == 0u || len > BRIDGE_FRAME_MAX) return -1;
    uint32_t mask = irq_save();
    uint32_t slot = g_tx_write;
    if (g_tx[slot].state != 0u) {
        irq_restore(mask);
        return -1;
    }
    g_tx[slot].state = 1u;
    g_tx_write = (slot + 1u) % BRIDGE_TX_SLOTS;
    g_tx[slot].handle = g_last_handle;
    g_tx[slot].session = g_last_session;
    g_tx[slot].len = len;
    canopus_memcpy(g_tx[slot].data, data, len);
    irq_restore(mask);
    __asm__ volatile("dmb" ::: "memory");
    g_tx[slot].state = 2u;
    return 0;
}

static int tx_pending(void)
{
    return g_tx[g_tx_read].state == 2u;
}

static void tx_drain(void)
{
    for (uint32_t i = 0; i < BRIDGE_TX_SLOTS; i++) {
        uint32_t slot = g_tx_read;
        if (g_tx[slot].state != 2u) return;
        if (((session_fn)L2_SESSION)(g_tx[slot].handle) == g_tx[slot].session) {
            if (l2_send_now(g_tx[slot].handle, g_tx[slot].session,
                            g_tx[slot].data, g_tx[slot].len) != 0) return;
        }
        __asm__ volatile("dmb" ::: "memory");
        g_tx[slot].state = 0u;
        g_tx_read = (slot + 1u) % BRIDGE_TX_SLOTS;
    }
}

/* ------------------------------------------------------------------ */
/* CanopusConnect                                                      */
/* ------------------------------------------------------------------ */

static int id_is_bridge(const uint8_t *id, uint32_t len)
{
    if (len != BRIDGE_ID_LEN) return 0;
    for (uint32_t i = 0; i < len; i++) {
        if (id[i] != (uint8_t)BRIDGE_ID[i]) return 0;
    }
    return 1;
}

static int32_t bridge_send_event(const struct canopus_context_v1 *ctx, uint32_t kind,
                                 const void *data, uint32_t len)
{
    (void)ctx;
    if (!g_listener || kind != CANOPUS_CONNECT_BINARY || data == 0 || len > 480u) return -1;
    uint8_t payload[2u + BRIDGE_ID_LEN + 480u];
    uint8_t event[BRIDGE_FRAME_MAX];
    payload[0] = (uint8_t)CANOPUS_CONNECT_BINARY;
    payload[1] = (uint8_t)BRIDGE_ID_LEN;
    canopus_memcpy(payload + 2u, BRIDGE_ID, BRIDGE_ID_LEN);
    canopus_memcpy(payload + 2u + BRIDGE_ID_LEN, data, len);
    int n = canopus_transport_v2_encode_event(CANOPUS_CMD_CONNECT_MESSAGE, payload,
                                              2u + BRIDGE_ID_LEN + len,
                                              event, sizeof(event));
    return n > 0 && tx_enqueue(event, (uint16_t)n) == 0 ? 0 : -1;
}

static int32_t bridge_listen(const struct canopus_context_v1 *ctx,
                             canopus_connect_callback_t cb, void *user_data)
{
    (void)ctx; (void)cb; (void)user_data;
    return 0;
}

static int bridge_remote_listening(const struct canopus_context_v1 *ctx)
{
    (void)ctx;
    return g_listener ? 1 : 0;
}

static const struct canopus_connect_obj g_connect_obj = {
    .send = bridge_send_event,
    .listen = bridge_listen,
    .remote_listening = bridge_remote_listening,
};

static void connect_response(const struct canopus_proto_request_v1 *req,
                             uint32_t result, uint8_t status)
{
    struct canopus_proto_response_v1 resp;
    uint8_t out[BRIDGE_FRAME_MAX];
    canopus_proto_response_init(&resp, req->request_id, result, 1u);
    int n = canopus_transport_v2_encode_response(&resp, req->command, &status, 1u,
                                                  out, sizeof(out));
    if (n > 0) (void)tx_enqueue(out, (uint16_t)n);
}

static void handle_cpc2(const uint8_t *data, uint32_t len)
{
    struct canopus_proto_request_v1 req;
    uint32_t off = 0;
    if (canopus_transport_v2_decode_request(data, len, &req, &off) != 0) return;
    const uint8_t *p = data + off;
    uint32_t plen = req.payload_size;

    if (req.command == CANOPUS_CMD_CONNECT_LISTEN) {
        if (plen < 2u) { connect_response(&req, CANOPUS_RESULT_REJECTED, CANOPUS_CONNECT_STATUS_FORMAT); return; }
        uint32_t id_len = p[1];
        if (id_len == 0u || id_len > CANOPUS_CONNECT_ID_MAX || plen < 2u + id_len ||
            (p[0] != 0u && p[0] != 1u)) {
            connect_response(&req, CANOPUS_RESULT_REJECTED, CANOPUS_CONNECT_STATUS_FORMAT);
            return;
        }
        if (p[0] == 0u) {
            g_listener = 0;
            connect_response(&req, CANOPUS_RESULT_COMPLETED, CANOPUS_CONNECT_STATUS_OK);
            return;
        }
        if (!id_is_bridge(p + 2u, id_len)) {
            connect_response(&req, CANOPUS_RESULT_DISALLOWED, CANOPUS_CONNECT_STATUS_MODULE);
            return;
        }
        g_listener = 1;
        connect_response(&req, CANOPUS_RESULT_COMPLETED, CANOPUS_CONNECT_STATUS_OK);
        return;
    }

    if (req.command == CANOPUS_CMD_CONNECT_SEND) {
        if (plen < 2u) { connect_response(&req, CANOPUS_RESULT_REJECTED, CANOPUS_CONNECT_STATUS_FORMAT); return; }
        uint32_t id_len = p[1];
        if (id_len == 0u || id_len > CANOPUS_CONNECT_ID_MAX || plen < 2u + id_len) {
            connect_response(&req, CANOPUS_RESULT_REJECTED, CANOPUS_CONNECT_STATUS_FORMAT);
            return;
        }
        if (!id_is_bridge(p + 2u, id_len)) {
            connect_response(&req, CANOPUS_RESULT_DISALLOWED, CANOPUS_CONNECT_STATUS_MODULE);
            return;
        }
        if (!g_listener) {
            connect_response(&req, CANOPUS_RESULT_DISALLOWED, CANOPUS_CONNECT_STATUS_NOT_LISTENING);
            return;
        }
        uint32_t kind = p[0];
        if (kind != CANOPUS_CONNECT_BINARY) {
            connect_response(&req, CANOPUS_RESULT_REJECTED, CANOPUS_CONNECT_STATUS_FORMAT);
            return;
        }
        uint32_t data_len = plen - 2u - id_len;
        if (data_len == 0u || data_len > 476u) {
            connect_response(&req, CANOPUS_RESULT_REJECTED, CANOPUS_CONNECT_STATUS_FORMAT);
            return;
        }
        connect_response(&req, CANOPUS_RESULT_COMPLETED, CANOPUS_CONNECT_STATUS_OK);
        bf_embed_dispatch(&g_ctx, p + 2u + id_len, data_len);
        return;
    }

    /* QueryDevice and other manager commands are intentionally unsupported.
     * AstroBox treats the missing window report as "keep the default 4". */
    struct canopus_proto_response_v1 resp;
    uint8_t out[BRIDGE_FRAME_MAX];
    canopus_proto_response_init(&resp, req.request_id, CANOPUS_RESULT_DISALLOWED, 0u);
    int n = canopus_transport_v2_encode_response(&resp, req.command, 0, 0, out, sizeof(out));
    if (n > 0) (void)tx_enqueue(out, (uint16_t)n);
}

/* ------------------------------------------------------------------ */
/* owner-loop pump                                                     */
/* ------------------------------------------------------------------ */

static void flush_logs(void)
{
    if (g_logged_hook) { g_logged_hook = 0; chaos_bridge_log("文件桥: L2 0x43 已接管"); }
    if (g_logged_rx) { g_logged_rx = 0; chaos_bridge_log("文件桥: 收到首个加密帧"); }
    if (g_logged_crypto) { g_logged_crypto = 0; chaos_bridge_log("文件桥: L2 解密失败"); }
    if (g_logged_send) { g_logged_send = 0; chaos_bridge_log("文件桥: L2 发送失败"); }
}

static int bt_timer_start(uint32_t period);

static void bt_tick(void *timer)
{
    (void)timer;
    if (!g_started) return;
    tx_drain();
    (void)bt_timer_start(tx_pending() ? 2u : 10u);
}

static int bt_timer_start(uint32_t period)
{
    void *loop = *(void *volatile *)L2_PROBE_BT_LOOP;
    if (loop == 0) return -1;
    if (g_bt_period == period) return 0;
    if (!g_bt_init) {
        if (((timer_init_fn)L2_OWNER_TIMER_INIT)(loop, g_bt_timer) != 0) return -1;
        g_bt_init = 1;
    }
    if (((timer_start_fn)L2_OWNER_TIMER_START)(g_bt_timer, bt_tick, period, period) != 0) return -1;
    g_bt_period = period;
    return 0;
}

static void l2_poll(void)
{
    l2_attach();
    flush_logs();
    for (uint32_t served = 0; served < BRIDGE_RX_SLOTS; served++) {
        uint32_t slot = g_rx_read;
        if (g_rx[slot].state != 2u) break;
        if (g_rx[slot].len != 0u &&
            ((session_fn)L2_SESSION)(g_rx[slot].handle) == g_rx[slot].session) {
            if (!g_connected || g_rx[slot].handle != g_last_handle ||
                g_rx[slot].session != g_last_session) {
                g_listener = 0;
            }
            g_last_handle = g_rx[slot].handle;
            g_last_session = g_rx[slot].session;
            g_connected = 1;
            handle_cpc2(g_rx[slot].data, g_rx[slot].len);
        }
        __asm__ volatile("dmb" ::: "memory");
        g_rx[slot].state = 0u;
        g_rx_read = (slot + 1u) % BRIDGE_RX_SLOTS;
    }
    if (tx_pending()) (void)bt_timer_start(2u);
}

static int owner_timer_start(uint32_t period, void (*tick)(void *))
{
    void *loop = *(void *volatile *)L2_OWNER_LOOP;
    if (loop == 0) return -1;
    if (!g_owner_init) {
        if (((timer_init_fn)L2_OWNER_TIMER_INIT)(loop, g_owner_timer) != 0) return -1;
        g_owner_init = 1;
    }
    return ((timer_start_fn)L2_OWNER_TIMER_START)(g_owner_timer, tick, period, period);
}

static void owner_tick(void *timer)
{
    (void)timer;
    if (g_started) l2_poll();
}

static void boot_tick(void *timer)
{
    (void)timer;
    if (owner_timer_start(10u, owner_tick) == 0) {
        g_started = 1;
        if (g_boot_timer != 0) {
            ((timer_delete_fn)L2_TIMER_DELETE)(g_boot_timer);
            g_boot_timer = 0;
        }
    }
    /* If the owner loop is not ready yet, keep polling from the bootstrap
     * timer. This is also the test path when the owner callback cannot be
     * installed from the module ctor. */
    l2_poll();
}

int chaos_bridge_start(void)
{
    if (g_started) return 0;
    // Resume: the owner timer was only slowed down on stop, not destroyed.
    if (g_owner_init) {
        if (owner_timer_start(10u, owner_tick) != 0) return -1;
        g_started = 1;
        if (g_bt_init) (void)bt_timer_start(10u);
        l2_attach();
        return 0;
    }
    if (g_boot_timer != 0) return 0;
    g_ctx.struct_size = sizeof(g_ctx);
    g_ctx.services = &g_services;
    g_ctx.fs = &g_fs_obj;
    g_ctx.kv = &g_kv_obj;
    g_ctx.connect = &g_connect_obj;
    g_ctx.target_id = "xiaomi-band-10-pro-3.101.043";
    g_ctx.module_id = BRIDGE_ID;
    g_services.struct_size = sizeof(g_services);
    g_services.group_version[CANOPUS_SVC_FS] = 2u;
    g_services.group_version[CANOPUS_SVC_KV] = 1u;
    g_services.group_version[CANOPUS_SVC_CONNECT] = 1u;
    g_boot_timer = ((timer_fn)L2_TIMER)(boot_tick, 10u, 0);
    return g_boot_timer != 0 ? 0 : -1;
}

/// Stop taking L2 0x43 and put the timers into a 1 Hz idle cadence. The static
/// timer objects stay initialized so a later start can resume without
/// reallocating; queued frames and the listener are dropped.
void chaos_bridge_stop(void)
{
    if (!g_started && g_boot_timer == 0) return;
    g_started = 0;
    l2_detach();
    if (g_boot_timer != 0) {
        ((timer_delete_fn)L2_TIMER_DELETE)(g_boot_timer);
        g_boot_timer = 0;
    }
    uint32_t mask = irq_save();
    for (uint32_t i = 0; i < BRIDGE_RX_SLOTS; i++) g_rx[i].state = 0u;
    for (uint32_t i = 0; i < BRIDGE_TX_SLOTS; i++) g_tx[i].state = 0u;
    g_rx_read = g_rx_write = 0u;
    g_tx_read = g_tx_write = 0u;
    g_connected = 0;
    g_listener = 0;
    g_last_handle = 0;
    g_last_session = 0;
    irq_restore(mask);
    if (g_owner_init) (void)owner_timer_start(1000u, owner_tick);
    if (g_bt_init) (void)bt_timer_start(1000u);
}
