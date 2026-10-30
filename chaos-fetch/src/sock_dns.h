#ifndef SOCK_DNS_H
#define SOCK_DNS_H

/* p67tc 3.101.043 —— socket DNS 传输层（v60 定案：bind+connect+recv(from=NULL)）
 *
 * ★ v56 定案（真机 v55 数据）：协议栈 TX 在这块 netdev 上**走不通** ——
 *   lpwork 的 `send()` 永久阻塞（sd_step=2、sd_sends=0、sd_stall_max=120），
 *   而两个超时其实都设上了（sd_orcv_rc=sd_osnd_rc=0，换算得 1 tick 非零）
 *   ⇒ 卡点不在"超时等待"，在协议栈的 TX 路上（DHCP 帧是我们直接写环的，
 *   这条 stack-TX 路径 v52 之前从未被使用过 ⇒ 从未被验证）。
 *   因此：**发送回归 raw 帧直接入环**（已验证能到手机），**接收仍用 socket**
 *   （bind 固定端口 53167，回包由协议栈按端口投递给本 socket）。
 *
 * 依据（两条独立证据）：
 *  1) 9Pro(n67) 固件的 lib_dnsquery.c 编译产物实测配方：
 *     socket → setsockopt(SO_RCVTIMEO=5s) → connect(服务器) → send → recv；
 *  2) bes1503 模拟器（真 p67tc 固件）动态验证（2026-10-16）：
 *     socket→fd=3 | setsockopt→0 | connect→0 | send→EAGAIN(11) | recv→ENETUNREACH(101)
 *     —— 全是网络层语义（模拟器无网络），零复位；参数类错误才会是 EINVAL。
 *
 * ★ 不用 sendto/recvfrom：该固件的 sendto 第 6 参按“指向 socklen 的指针”语义
 *   （`ldr r1,[r2]`；按值传 16 ⇒ msg_namelen=0 ⇒ EINVAL），recvfrom 第 6 参按值。
 *   connect + send/recv 两个包装的 ABI 无歧义（4 参），且回包被内核按四元组过滤
 *   —— 顺带消灭了 v41 那套“宽容匹配”的全部不确定性。
 *
 * 上下文约束（重要）：socket 的 fd 属于**当前任务组**的 fd 表。本模块只在
 * drain worker（work queue 0 的固定线程）里建/用 socket，绝不在陷阱或异常
 * 上下文里碰它，保证 fd 表始终是同一张。
 *
 * 缓冲：发送负载复用 g_dns.tx_frame 的负载区（raw 帧 = [IPv4 20B][UDP 8B][DNS]），
 * 接收复用 g_rxbuf（两处都在 net_lock 下串行，不会互相踩）；.bss 只增加本结构。
 */

#include <stddef.h>
#include <stdint.h>

/* ★★★ 真机 2026-10-16 17:30 事故定案：这里必须是**带 Thumb 位的奇数地址**。
 * 曾写成偶数（0x0C1CAD34…）⇒ BX 过去进 ARM 态 ⇒ UsageFault（CFSR=0x20000 INVSTATE，
 * PC=0x0C1CAD34、task=lpwork 的 panic）⇒ 看门狗复位。
 * 模拟器验证时用的是奇数地址（0x0C1CAD35…）所以没暴露 —— 教训：
 * **模块源码里的调用目标地址一律 |1**（门：tools/audit_thumb.py，源码+产物双查）。 */
#define SD_FN_SOCKET      (0x0C1CAD34u | 1u)   /* int socket(int,int,int) */
#define SD_FN_SETSOCKOPT  (0x0C1FDAB4u | 1u)   /* int setsockopt(int,int,int,void*,int) */
#define SD_FN_CONNECT     (0x0C1FD228u | 1u)   /* int connect(...) —— v56 不再使用 */
#define SD_FN_BIND        (0x0C1FD120u | 1u)   /* int bind(int,const struct sockaddr*,int) */
#define SD_FN_SEND        (0x0C1FD500u | 1u)   /* int send(int,const void*,int,int) */
/* ★9PRO-SOCKET: sendto 真实入口 = 0x0C1FD664（0x0C1FD665 是中间偏移，别用！）
 * int sendto(int fd, const void *buf, int len, int flags,
 *            const struct sockaddr *to, int tolen) */
#define SD_FN_SENDTO      (0x0C1FD664u | 1u)
#define SD_FN_RECV        (0x0C1FD64Cu | 1u)   /* int recv(int,void*,int,int) —— v56/v57 用 */
/* ★★★ v61 地址纠正（模拟器三路对照定案）：**0x0C1FD570 才是 recvfrom**
 *   （对第 6 参做 `ldr r1,[r2]` 解引用 ⇒ fromlen 是**指针**，POSIX 收包语义）
 *   0x0C1FD664 = **sendto**（fromlen 按值用）—— v56~v60 一直把它当 recv 用，
 *   于是真机的"sd_recv_rc=512/无拷贝/ENOTCONN"其实是**发了 512 字节出去** ✗✗。
 *   另有 0x0C1FD500 = recv 包装(fd,buf,len,flags → name=NULL/len=NULL) 也可用（实测 EAGAIN ✓）。*/
#define SD_FN_RECVFROM    (0x0C1FD570u | 1u)   /* int recvfrom(fd,buf,len,flags,from,fromlen*) */
#define SD_FN_ERRNO_PTR   (0x0C1E45BCu | 1u)   /* int *__errno(void) */

#define SD_AF_INET        2
#define SD_SOCK_DGRAM     2
#define SD_SOL_SOCKET     1
/* ★★★ 选项号/长度：以**本固件自己的 setsockopt 跳表**为准（模拟器是另一套 NuttX 配置，
 * 选项号不可照搬！真机 2026-10-16 定案）：
 *   p67tc setsockopt @0C1FDAB4：`cmp opt,#0xf` + 跳表@0C1FDB50
 *     opt=0xa → handler C：`cmp len,#0x10` 严格 16B，存 psock+0x18（接收超时）
 *     opt=0xe → handler C：同一处理器，存 psock+0x1a（发送超时）
 *     opt=0xb → handler A：**4 字节布尔标志**（不是 SNDTIMEO！）
 *   9Pro(n67) 能跑通的 lib_dnsquery 客户端也正好是：
 *     setsockopt(fd, 1, 0xa, &tv{5,0}, 16) + setsockopt(fd, 1, 0xe, &tv{5,0}, 16)
 *   ⇒ 之前写成 SNDTIMEO=0xb / len=8 ⇒ 两个都 EINVAL ⇒ 发送无限等 TX 缓冲 ⇒ lpwork 卡死。 */
#define SD_SO_RCVTIMEO    0x0Au
#define SD_SO_SNDTIMEO    0x0Eu
#define SD_SERVER_PORT    53u
/* timeval 是 **16 字节**（64 位字段），长度必须精确 0x10 —— 布局：{tv_sec(8B), tv_usec(8B)} */
#define SD_TIMEVAL_SIZE   16u
/* ★ v59：运行时收包用 MSG_DONTWAIT（固件 flags 校验掩码 0xFFF7F7F0 ⇒ 合法位
 * 0x0008080F；0x4/0x8 都在合法集内）。lpwork 被固件抢用时绝不能阻塞在收包上。*/
#define SD_MSG_DONTWAIT   0x04
/* ★9PRO-SOCKET: 9Pro 考古配方 —— 5s 超时（原 100ms 太短，
 * 真机上一次 RTT 能到 6194 拍 ≈ 数秒，100ms 必然提前 EAGAIN）。*/
#define SD_RCV_TIMEO_MS   5000u
#define SD_SND_TIMEO_MS   5000u
/* 9Pro 收缓冲 512 字节 */
#define SD_RECV_BUF       512
/* 9Pro 重试 3 次 */
#define SD_RETRY_MAX      3u
/* 一拍里最多试几次 recv（回包常在毫秒级，多试两次能当拍就完成） */
#define SD_POLL_TRIES     2u

typedef int   (*sd_fn_socket)(int,int,int);
typedef int   (*sd_fn_setsockopt)(int,int,int,void*,int);
typedef int   (*sd_fn_bind)(int,const void*,int);
typedef int   (*sd_fn_connect)(int,const void*,int);
typedef int   (*sd_fn_send)(int,const void*,int,int);
/* ★9PRO-SOCKET: NuttX sendto —— to 指针与 tolen 都从栈上传（见 patch 头注的 ABI） */
typedef int   (*sd_fn_sendto)(int,const void*,int,int,const void*,int);
typedef int   (*sd_fn_recv)(int,void*,int,int);
typedef int   (*sd_fn_recvfrom)(int,void*,int,int,void*,void*);   /* v61: fromlen 是指针 */
typedef int * (*sd_fn_errnop)(void);

struct sock_dns {
    int      fd;            /* <0 = 未建立 */
    int      last_errno;
    int      open_rc, so_rcv_rc, so_snd_rc, bind_rc, conn_rc;  /* 建链各步原始返回 */
    int      send_rc, recv_rc;          /* 最近一次收发的原始返回 */
    uint16_t port;
    /* ★9PRO-SOCKET */
    uint32_t sendto_rc;      /* 最近一次 sendto 返回值 */
    uint32_t sendto_n;       /* sendto 累计调用次数 */
    uint32_t retries;        /* 9Pro 重试计数 */
    uint32_t retry_hits;     /* 第 N 次重试才成功 */
    uint8_t  auto_port;      /* 1 = 不 bind，用内核分配的源端口 */                      /* bind 的本地端口（= raw 帧源端口）*/
    uint8_t  bound;                     /* bind 成功过 */
    uint8_t  connected;                 /* ★v60：connect 成功过（from=NULL 收包的前提）*/
    uint8_t  pad2;
    uint32_t opens, binds, connects, tx_enq;   /* tx_enq = 走 raw 入环的查询数 */
    uint32_t sends, recvs, ready;       /* ready = 收到数据报的次数 */
    uint32_t err_sock, err_opt, err_bind, err_conn, err_send, err_recv;
    uint8_t  enabled;                   /* 0 = 回退 raw TUN 传输 */
    uint8_t  step;                      /* 卡住判据：0=空闲 1=建链 2=send 3=recv */
    uint8_t  flag_extra;                /* 附加 recvfrom flags（自检=0 走阻塞版）*/
    uint8_t  server[4];                 /* 当前 connect 的对端 */
    uint8_t  pad[1];
};

static inline int sd_errno_get(void)
{
    int *p = ((sd_fn_errnop)(uintptr_t)SD_FN_ERRNO_PTR)();
    return (p != (int *)0) ? *p : -1;
}

/* 把两个超时都设上（16 字节 timeval + 精确长度）。
 * 返回 0 = 两个都成功；非 0 = 至少一个失败（调用方应视为"不可用"直接失败，
 * 因为**没有超时的 socket 一旦阻塞会永久卡住 worker** —— 真机 v54 就这么死的）。 */
static inline int sd_set_timeouts(struct sock_dns *s)
{
    int32_t tv[4];
    int rc = 0;
    /* {tv_sec = 0, tv_usec = 100ms}，按 64 位字段布局（低字在前，小端）*/
    tv[0] = 0; tv[1] = 0;                                  /* tv_sec  */
    tv[2] = (int32_t)(SD_RCV_TIMEO_MS * 1000u); tv[3] = 0; /* tv_usec */
    s->so_rcv_rc = ((sd_fn_setsockopt)(uintptr_t)SD_FN_SETSOCKOPT)(
        s->fd, SD_SOL_SOCKET, SD_SO_RCVTIMEO, (void *)tv, (int)SD_TIMEVAL_SIZE);
    if (s->so_rcv_rc != 0) { rc = -1; ++s->err_opt; }
    tv[2] = (int32_t)(SD_SND_TIMEO_MS * 1000u);
    s->so_snd_rc = ((sd_fn_setsockopt)(uintptr_t)SD_FN_SETSOCKOPT)(
        s->fd, SD_SOL_SOCKET, SD_SO_SNDTIMEO, (void *)tv, (int)SD_TIMEVAL_SIZE);
    if (s->so_snd_rc != 0) { rc = -1; ++s->err_opt; }
    if (rc != 0) s->last_errno = sd_errno_get();
    return rc;
}

/* 清错误计数（cmd15；由 lpwork 调用，不在调用者线程里碰 socket）。 */
static inline void sd_reset_stats(struct sock_dns *s)
{
    s->open_rc = s->so_rcv_rc = s->so_snd_rc = s->bind_rc = 0;
    s->send_rc = s->recv_rc = 0;
    /* ★9PRO-SOCKET */
    s->sendto_rc = 0; s->retries = 0u; s->retry_hits = 0u;
    s->last_errno = 0;
    s->err_sock = s->err_opt = s->err_bind = s->err_send = s->err_recv = 0u;
}

/* sockaddr_in 布局（NuttX：family 主机序 16 位、port 网络序 16 位、addr 4B、余 0）。
 * 单独抽出来是为了能在宿主机上直接验证字节布局（tools/_host_sock_smoke.c）。 */
static inline void sd_build_sa(uint8_t sa[16], const uint8_t ip[4], uint16_t port)
{
    int i;
    for (i = 0; i < 16; ++i) sa[i] = 0u;
    sa[0] = (uint8_t)(SD_AF_INET & 0xFFu);      /* sin_family（主机序，小端） */
    sa[1] = (uint8_t)(SD_AF_INET >> 8);
    sa[2] = (uint8_t)(port >> 8);               /* sin_port：网络序 */
    sa[3] = (uint8_t)(port & 0xFFu);
    for (i = 0; i < 4; ++i) sa[4 + i] = ip[i];
}

/* 建立/复用 socket 并 **bind 到固定端口**（= raw 帧的源端口）。
 * 发送不经协议栈（raw 帧直接入环），这里只为"收"服务：回包由协议栈按目的端口投递。
 * 返回 0 = 可用；-2 socket 失败 / -4 超时设不上（recv 必须有时限）/ -5 bind 失败。 */
static inline int sd_open(struct sock_dns *s, uint16_t port, const uint8_t server[4])
{
    static const uint8_t any_ip[4] = { 0u, 0u, 0u, 0u };
    uint8_t sa[16];
    int rc;

    if (s == (struct sock_dns *)0) return -1;
    s->port = port;

    if (s->fd < 0) {
        s->step = 1u;
        rc = ((sd_fn_socket)(uintptr_t)SD_FN_SOCKET)(SD_AF_INET, SD_SOCK_DGRAM, 0);
        s->step = 0u;
        s->open_rc = rc;
        if (rc < 0) {
            s->last_errno = sd_errno_get();
            ++s->err_sock;
            return -2;
        }
        s->fd = rc;
        ++s->opens;
        s->bound = 0u;
    }
    /* 没有超时的 socket 不能用于阻塞 recv ⇒ 每次复核，设不上就判失败 */
    if (s->so_rcv_rc != 0 || s->so_snd_rc != 0) {
        s->so_rcv_rc = 0; s->so_snd_rc = 0;
        if (sd_set_timeouts(s) != 0) return -4;
    }
    /* ★9PRO-SOCKET: **默认不 bind**。
     * 9Pro 考古：miwear 用 sendto（每帧自带目的地址），源端口由内核分配。
     * 我们 v61 之前 bind(0.0.0.0:53167) 固定源端口，真机疑似被手机侧
     * 按四元组丢帧（⇒ v49 定案"69 查询仅 2 回包"）。改成不 bind 后
     * 源端口随机，反而更接近真机行为。
     * 代价：回包不再按固定端口投递 ⇒ 收包必须靠 recvfrom + DNS 负载判别
     *      （这本来就是 consume_payload 做的事）。 */
    if (!s->bound && !s->auto_port) {
        sd_build_sa(sa, any_ip, port);
        s->step = 1u;
        rc = ((sd_fn_bind)(uintptr_t)SD_FN_BIND)(s->fd, (const void *)sa, 16);
        s->step = 0u;
        s->bind_rc = rc;
        if (rc != 0) {
            s->last_errno = sd_errno_get();
            ++s->err_bind;
            return -5;
        }
        s->bound = 1u;
        ++s->binds;
    } else if (!s->bound && s->auto_port) {
        s->bound = 1u;   /* 视作就绪（未 bind，但 auto_port 模式不需要） */
    }
    /* ★v61：connect 不再是必需（真 recvfrom 走 from≠NULL 路径，不需要对端）。
     * 保留这段代码但默认不调用（server 传 NULL）—— 万一将来要用四元组过滤再说。
     * 副作用：不 connect ⇒ 手机侧改写回包源地址也不会被内核按对端过滤掉 ✓。*/
    if (server != (const uint8_t *)0) {
        int i;
        for (i = 0; i < 4; ++i) s->server[i] = server[i];
        if (!s->connected) {
            sd_build_sa(sa, server, (uint16_t)SD_SERVER_PORT);
            s->step = 1u;
            rc = ((sd_fn_connect)(uintptr_t)SD_FN_CONNECT)(s->fd, (const void *)sa, 16);
            s->step = 0u;
            s->conn_rc = rc;
            if (rc != 0) {
                s->last_errno = sd_errno_get();
                ++s->err_conn;
                return -6;
            }
            s->connected = 1u;
            ++s->connects;
        }
    }
    return 0;
}

/* 发一个 DNS 负载（内核负责 IPv4/UDP 头）。>0 = 已发出字节数。 */
static inline int sd_send(struct sock_dns *s, const void *buf, size_t len)
{
    int rc;
    if (s == (struct sock_dns *)0 || s->fd < 0 || buf == (const void *)0) return -1;
    s->step = 2u;
    rc = ((sd_fn_send)(uintptr_t)SD_FN_SEND)(s->fd, buf, (int)len, 0);
    s->step = 0u;
    s->send_rc = rc;
    if (rc < 0) {
        s->last_errno = sd_errno_get();
        ++s->err_send;
        return rc;
    }
    ++s->sends;
    return rc;
}

static inline int sd_recv(struct sock_dns *s, void *buf, size_t cap);
static inline int sd_send(struct sock_dns *s, const void *buf, size_t len);

/* ★9PRO-SOCKET: 9Pro 的发送方式 —— sendto，**不需要 connect**。
 * ABI（反汇编 0x0C1FD664 定案）：to 指针 + tolen 走栈，
 * 所以这里按 (fd, buf, len, flags, to, tolen) 六个参数调；
 * tolen 按**值**传（与 sendto 的标准语义一致）。
 * server 为 NULL 时退回 send()（保持与旧路径兼容）。 */
static inline int sd_sendto(struct sock_dns *s, const void *buf, size_t len,
                            const uint8_t server[4], uint16_t server_port)
{
    uint8_t sa[16];
    int rc;
    if (s == (struct sock_dns *)0 || s->fd < 0 || buf == (const void *)0) return -1;
    if (server == (const uint8_t *)0) {
        return sd_send(s, buf, len);
    }
    sd_build_sa(sa, server, server_port);
    s->step = 2u;
    rc = ((sd_fn_sendto)(uintptr_t)SD_FN_SENDTO)(
        s->fd, buf, (int)len, 0, (const void *)sa, 16);
    s->step = 0u;
    s->send_rc = rc;
    s->sendto_rc = (uint32_t)(rc < 0 ? rc : 0);
    ++s->sendto_n;
    if (rc < 0) {
        s->last_errno = sd_errno_get();
        ++s->err_send;
        return rc;
    }
    ++s->sends;
    return rc;
}

/* ★9PRO-SOCKET: 9Pro 的"发+等回包"完整一轮（重试 3 次）。
 * 与模块的看门狗解耦：单次 recv 最多 5s，3 次共 15s。
 * 返回收到的字节数（>0），或最后一次的错误码。 */
static inline int sd_round(struct sock_dns *s, const void *query, size_t qlen,
                           const uint8_t server[4], uint16_t server_port,
                           void *rbuf, size_t rcap)
{
    unsigned attempt;
    int rc = -1;
    for (attempt = 0u; attempt < SD_RETRY_MAX; ++attempt) {
        int sent = sd_sendto(s, query, qlen, server, server_port);
        if (sent > 0) {
            s->retries += attempt;                 /* 统计额外重试次数 */
            if (attempt) ++s->retry_hits;
        }
        rc = sd_recv(s, rbuf, rcap);
        if (rc > 0) return rc;                     /* 拿到回包 */
        /* 收不到 ⇒ 下一轮重发（9Pro 行为） */
    }
    return rc;
}

/* ★★★ v58 定案：**未 connect 的 UDP socket 上不能用 `recv()`** ——
 * recv 等价于 recvfrom(fd,buf,len,flags,NULL,0)，NuttX 在"名字为 NULL 且未连接"时
 * 直接返回 ENOTCONN(-107)（真机 6037 次全 ENOTCONN，一个包都收不到）。
 * 正解：带 from 缓冲的 recvfrom —— 第 5 参是 sockaddr*（指针），
 * **第 6 参在本固件是按值**（反汇编：`str r5,[r7,#0x14]` 无解引用；且 fromlen!=0 && from==0 ⇒ EINVAL）。
 * 这样既不用 connect（对手机侧改写源地址更宽容），又能正常收包。 */
static uint8_t sd_from_buf[16];
static inline int sd_recv(struct sock_dns *s, void *buf, size_t cap)
{
    int rc;
    if (s == (struct sock_dns *)0 || s->fd < 0 || buf == (void *)0) return -1;
    /* ★v61：规范形状 —— from 缓冲（family 预置 AF_INET）+ **fromlen 是"指向 16 的指针"**。
     * flags=0：实测 1 tick(10ms) 的 SO_RCVTIMEO 会正常触发 EAGAIN ⇒ 无需 DONTWAIT，也不怕它语义不明。*/
    uint32_t fl = (uint32_t)sizeof(sd_from_buf);
    sd_from_buf[0] = (uint8_t)SD_AF_INET;
    sd_from_buf[1] = 0u;
    s->step = 3u;
    rc = ((sd_fn_recvfrom)(uintptr_t)SD_FN_RECVFROM)(
        s->fd, buf, (int)cap, (int)s->flag_extra,
        (void *)sd_from_buf, (void *)&fl);
    s->step = 0u;
    s->recv_rc = rc;
    ++s->recvs;
    if (rc < 0) {
        s->last_errno = sd_errno_get();
        ++s->err_recv;
        return rc;
    }
    ++s->ready;
    return rc;
}

/* 丢弃残留数据报（切换查询/服务器后清队列）：最多丢 max 个。 */
static inline unsigned sd_drain(struct sock_dns *s, void *buf, size_t cap, unsigned max)
{
    unsigned n = 0u;
    while (n < max) {
        if (sd_recv(s, buf, cap) <= 0) break;
        ++n;
    }
    return n;
}

#endif /* SOCK_DNS_H */
