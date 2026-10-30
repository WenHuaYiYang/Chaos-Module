#ifndef TCP_DNS_H
#define TCP_DNS_H

/* tcp_dns.h —— TCP-DNS（RFC 7766，俗称 DoT/Do53 里的明文那半）的最小客户端。
 *
 * 为什么做它（2026-10-16 真机数据驱动）：
 *   手机对 UDP/53 的转发是**零星到达**（v65：34 查询收到 20 个 sport:53 帧、只成 2 次），
 *   而同一链路上的 **TCP/HTTP 是稳的**（fetch 的 HTTP over TCP 每次都通 ✓）。
 *   9Pro 的 lib_dnsquery 也印证了这个判断：它按传输分支用
 *   **16 字节纯 DNS（UDP）/ 28 字节 = 2 字节长度前缀 + 26 字节 DNS（TCP）** ✓。
 *
 * 帧格式（RFC 7766 §8）：
 *     请求/应答 = [len_hi][len_lo][DNS 报文]
 * 端口仍是 53；只是先 TCP 三次握手，之后每条消息自带 2 字节长度。
 */

#include <stddef.h>
#include <stdint.h>

#define TD_AF_INET    2      /* 本固件实测：socket(2, 2, 0) → UDP 可用 ⇒ AF_INET=2 ✓ */
#define TD_SOCK_STREAM 1
#define TD_SOCK_DGRAM  2
#define TD_PORT        53u
#define TD_HDR_LEN     2u

/* 填一个 IPv4 sockaddr（family / port 网络序 / addr） */
static inline int td_build_sa(uint8_t *sa, const uint8_t ip[4], uint16_t port)
{
    int i;
    if (sa == (uint8_t *)0 || ip == (const uint8_t *)0) return -1;
    for (i = 0; i < 16; ++i) sa[i] = 0u;
    sa[0] = (uint8_t)TD_AF_INET; sa[1] = 0u;
    sa[2] = (uint8_t)(port >> 8); sa[3] = (uint8_t)(port & 0xFFu);
    for (i = 0; i < 4; ++i) sa[4 + i] = ip[i];
    return 16;
}

/* 给 DNS 负载加 2 字节长度前缀（网络序）。返回总长度。 */
static inline size_t td_frame(uint8_t *out, size_t cap, const uint8_t *dns, size_t dns_len)
{
    if (out == (uint8_t *)0 || dns == (const uint8_t *)0) return 0u;
    if (cap < TD_HDR_LEN + dns_len) return 0u;
    out[0] = (uint8_t)(dns_len >> 8);
    out[1] = (uint8_t)(dns_len & 0xFFu);
    { size_t i; for (i = 0u; i < dns_len; ++i) out[TD_HDR_LEN + i] = dns[i]; }
    return TD_HDR_LEN + dns_len;
}

#endif /* TCP_DNS_H */
