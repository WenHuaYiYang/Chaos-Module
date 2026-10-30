#ifndef DNS_REPLY_H
#define DNS_REPLY_H

/* dns_reply.h —— 自造一条 DNS 回包（IPv4 + UDP + DNS），用来做"注入式"收包链自检。
 *
 * 为什么需要（2026-10-16 真机教训链）：
 *   真机上手机侧是否转发 UDP/53 回包是**唯一无法在模拟器里复现**的事实；但除了它，
 *   整条 RX 链（协议栈按端口投递 → socket 收 → ID/问题名校验 → 写缓存）都能自证。
 *   本模块于是自己造一条回包、从**手机同一个入口**（NETDEV_INPUT → IPV4_CALLBACK）
 *   注入协议栈，再用 socket 收回来验：这一段与"手机转发"解耦，设备上一条命令即可复现。
 *
 * 全部字节级手写：IPv4 头 + UDP 头 + DNS 应答，校验和按 RFC 1071 计算。
 */

#include <stddef.h>
#include <stdint.h>

#define DR_SRC_IP0   114u
#define DR_SRC_IP1   114u
#define DR_SRC_IP2   114u
#define DR_SRC_IP3   114u
#define DR_SERVER_PORT 53u

static inline uint16_t dr_csum16(const uint8_t *d, size_t n)
{
    uint32_t s = 0u;
    size_t i;
    while ((n & 1u) != 0u) { s += (uint32_t)d[n] << 8; ++n; }   /* 末字节按高字节算 */
    for (i = 0; i < n; i += 2u) s += ((uint32_t)d[i] << 8) | d[i + 1];
    while ((s >> 16) != 0u) s = (s & 0xFFFFu) + (s >> 16);
    return (uint16_t)(~s & 0xFFFFu);
}

static inline uint16_t dr_csum_fold(uint32_t s)
{
    while ((s >> 16) != 0u) s = (s & 0xFFFFu) + (s >> 16);
    return (uint16_t)(~s & 0xFFFFu);
}

/* 造 DNS 应答负载：头(12) + 问题段(照抄) + 答案(A 记录一条, name 用 0xC00C 指针) */
static inline size_t dr_build_reply(uint8_t *out, size_t cap,
                                   const char *qname, uint16_t qid,
                                   uint32_t ip, uint32_t ttl)
{
    size_t n = 0u, i;
    unsigned labels = 0u;
    /* 1) 问题段：逐标签拷贝 + 结束符 + type/class */
    for (i = 0u; qname[i] != '\0' && labels < 8u; ++i) {
        size_t l = 0u, j = i;
        while (qname[j] != '.' && qname[j] != '\0') { ++l; ++j; }
        if (l == 0u || l > 63u) return 0u;
        if (n + 1u + l > cap) return 0u;
        out[n] = (uint8_t)l; ++n;                /* ★ 长度字节写完必须 ++n，否则被首字符覆盖 */
        for (size_t k = 0u; k < l; ++k) { out[n] = (uint8_t)qname[i + k]; ++n; }
        ++labels;
        i = j;                                   /* 跳过 '.' */
    }
    if (labels == 0u) return 0u;
    if (n + 5u > cap) return 0u;
    out[n++] = 0u;                              /* 名字结束 */
    out[n++] = 0x00; out[n++] = 0x01;            /* QTYPE=A */
    out[n++] = 0x00; out[n++] = 0x01;            /* QCLASS=IN */
    if (n + 16u > cap) return 0u;

    /* 2) 头部回填：把问题段挪到头后面（★重叠拷贝必须**从后往前**，否则自我覆盖） */
    {
        size_t qlen_total = n;
        size_t hdr = 12u;
        for (i = n; i > 0u; --i) out[i + hdr - 1u] = out[i - 1u];
        out[0] = (uint8_t)(qid >> 8); out[1] = (uint8_t)(qid & 0xFF);
        out[2] = 0x81; out[3] = 0x80;             /* QR=1 RD=1 RA=1 */
        out[4] = 0x00; out[5] = 0x01;             /* QDCOUNT=1 */
        out[6] = 0x00; out[7] = 0x01;             /* ANCOUNT=1 */
        out[8] = 0x00; out[9] = 0x00; out[10] = 0x00; out[11] = 0x00;
        n = hdr + qlen_total;
    }

    /* 3) 答案：name 指针 + A + IN + ttl + rdlen + ip */
    if (n + 16u > cap) return 0u;
    out[n++] = 0xC0; out[n++] = 0x0C;            /* NAME = ptr 0x0C */
    out[n++] = 0x00; out[n++] = 0x01;            /* TYPE=A */
    out[n++] = 0x00; out[n++] = 0x01;            /* CLASS=IN */
    out[n++] = (uint8_t)(ttl >> 24); out[n++] = (uint8_t)(ttl >> 16);
    out[n++] = (uint8_t)(ttl >> 8);  out[n++] = (uint8_t)ttl;
    out[n++] = 0x00; out[n++] = 0x04;            /* RDLENGTH=4 */
    out[n++] = (uint8_t)(ip >> 24); out[n++] = (uint8_t)(ip >> 16);
    out[n++] = (uint8_t)(ip >> 8);  out[n++] = (uint8_t)ip;
    return n;
}

/* 组完整 IPv4+UDP 帧：src=114.114.114.114:53 → dst=<dst_ip>:53167 */
static inline size_t dr_build_frame(uint8_t *out, size_t cap,
                                    uint32_t dst_ip, uint16_t dport,
                                    const uint8_t *dns, size_t dns_len)
{
    uint32_t sum;
    uint16_t c;
    size_t n = 0u, udp_len = 8u + dns_len;
    if (cap < 20u + udp_len) return 0u;
    /* --- IPv4 头 --- */
    out[0] = 0x45; out[1] = 0x00;
    out[2] = (uint8_t)((20u + udp_len) >> 8); out[3] = (uint8_t)(20u + udp_len);
    out[4] = 0x12; out[5] = 0x34;               /* ID */
    out[6] = 0x00; out[7] = 0x00;               /* flags/frag */
    out[8] = 0x40;                              /* TTL = 64 */
    out[9] = 0x11;                              /* proto = UDP（★[9] 是 proto、[10..11] 才是校验和） */
    out[10] = 0x00; out[11] = 0x00;             /* 校验和先置 0，后填 */
    out[12] = DR_SRC_IP0; out[13] = DR_SRC_IP1; out[14] = DR_SRC_IP2; out[15] = DR_SRC_IP3;
    out[16] = (uint8_t)(dst_ip >> 24); out[17] = (uint8_t)(dst_ip >> 16);
    out[18] = (uint8_t)(dst_ip >> 8);  out[19] = (uint8_t)dst_ip;
    c = dr_csum16(out, 20u);
    out[10] = (uint8_t)(c >> 8); out[11] = (uint8_t)c;
    /* --- UDP 头 --- */
    out[20] = 0x00; out[21] = DR_SERVER_PORT;    /* sport=53 */
    out[22] = (uint8_t)(dport >> 8); out[23] = (uint8_t)dport;
    out[24] = (uint8_t)(udp_len >> 8); out[25] = (uint8_t)udp_len;
    out[26] = 0x00; out[27] = 0x00;             /* csum=0(后填) */
    n = 28u;
    { size_t i; for (i = 0u; i < dns_len; ++i) out[n++] = dns[i]; }
    /* --- UDP 校验和（含伪头）--- */
    sum = 0u;
    {   uint8_t ph[12];
        ph[0] = DR_SRC_IP0; ph[1] = DR_SRC_IP1; ph[2] = DR_SRC_IP2; ph[3] = DR_SRC_IP3;
        ph[4] = out[16]; ph[5] = out[17]; ph[6] = out[18]; ph[7] = out[19];
        ph[8] = 0x00; ph[9] = 17;
        ph[10] = (uint8_t)(udp_len >> 8); ph[11] = (uint8_t)udp_len;
        { size_t i; for (i = 0u; i < 12u; i += 2u) sum += ((uint32_t)ph[i] << 8) | ph[i + 1]; }
    }
    { size_t i; for (i = 0u; i < n; i += 2u) sum += ((uint32_t)out[i] << 8) | out[i + 1]; }
    c = dr_csum_fold(sum);
    if (c == 0u) c = 0xFFFFu;
    out[26] = (uint8_t)(c >> 8); out[27] = (uint8_t)c;
    return n;
}

#endif /* DNS_REPLY_H */
