#ifndef STANDALONE_DNS_H
#define STANDALONE_DNS_H

#include <stddef.h>
#include <stdint.h>

#include "dns_codec.h"
#include "raw_udp.h"

/* Pure protocol state machine. It never opens files, sockets, devices or
 * threads. The caller supplies the completed TUN frame to consume(). */
#define STANDALONE_DNS_SOURCE_PORT 53167u
#define STANDALONE_DNS_SERVER_PORT 53u
#define STANDALONE_DNS_MAX_QUERY (DNS_CODEC_MAX_NAME + 18u)
#define STANDALONE_DNS_MAX_FRAME (RAW_UDP_HEADER_SIZE + STANDALONE_DNS_MAX_QUERY)

enum standalone_dns_phase {
    STANDALONE_DNS_IDLE = 0,
    STANDALONE_DNS_WAIT_REPLY = 1,
    STANDALONE_DNS_DONE = 2,
    STANDALONE_DNS_FAILED = 3
};

enum standalone_dns_consume {
    STANDALONE_DNS_IGNORED = 0,
    STANDALONE_DNS_CONSUMED = 1,
    STANDALONE_DNS_FAILED_FRAME = -1
};

/* ---- v41：回包匹配的诊断 + 宽容化（2026-10-16 真机定案）-----------------
 * 真机数据：dns_sent=11、dns_replies=1、dns_ignored=8；原匹配要求
 *   源 IP==114.114.114.114 且 目的 IP==本机 且 源端口==53 且 目的端口==53167 且 ID 相等。
 * 手机侧转发（AstroBox/netproxy）很可能改写 IP/端口（甚至代答）⇒ 严格匹配整片误杀。
 * 新接受条件（仍防误配，靠"最近查询 ID 环 + 问题名逐字节匹配"两道闸）：
 *   可解析 UDP + 源端口 53 + 负载≥2 + ID 在最近 8 次查询里 + 负载问题名==当前待解析名。
 * 原来的严格四项**只记录不拒绝**（strict_ok / loose_accepts / last_ign / 各项 ignored 计数）
 * —— 报告一眼看出"到底被改写了哪一项"。 */
struct standalone_dns_diag {
    uint16_t id_hist[8];
    uint8_t  id_n, id_pos;
    uint32_t ign_idle, ign_parse, ign_port, ign_id, ign_qname, loose_accepts, strict_ok;
    uint16_t last_ign_id, last_ign_sport, last_ign_dport, last_ign_status;
    uint8_t  last_ign_sip[4], last_ign_dip[4];
    /* ★ v50：解析失败的**精确原因直方图**（index = -rc：3=BAD_IPV4 4=BAD_LENGTH
     * 5=FRAGMENT 6=NOT_UDP 7=IP_CHECKSUM 8=UDP_CHECKSUM）——一眼分出"TCP 噪声"与"坏回包" */
    uint32_t parse_fail_hist[10];
    /* ★ v50：无解析旁路探测 —— raw_udp_parse 失败的帧也按"最朴素"方式看形状：
     * look_dns=像 DNS 回包（0x45/udp/源端口53） look_udp=是 UDP 但端口不对 look_other=其它 */
    uint32_t look_dns, look_udp, look_other;
    /* ★ v50：原始帧抓样（宿主机离线重放）。prob=最后一次被无视/被拒的帧；
     * good=最后一次被消费的帧。kind: 1=UDP解析失败 2=端口不符 3=ID不在环
     * 4=问题名不符 5=编解码拒绝 6=不在等待态 */
    uint8_t  prob_bytes[64], good_bytes[64];
    uint16_t prob_len, good_len;
    int32_t  prob_status, good_status;
    uint8_t  prob_kind;
};
static inline void standalone_dns_diag_push(struct standalone_dns_diag *d, uint16_t id) {
    if (d == (struct standalone_dns_diag *)0) return;
    d->id_hist[d->id_pos & 7u] = id;
    d->id_pos = (uint8_t)((d->id_pos + 1u) & 7u);
    if (d->id_n < 8u) ++d->id_n;
}
static inline int standalone_dns_diag_recent(const struct standalone_dns_diag *d, uint16_t id) {
    uint8_t i;
    if (d == (const struct standalone_dns_diag *)0 || d->id_n == 0u) return 0;
    for (i = 0u; i < d->id_n; ++i) if (d->id_hist[i] == id) return 1;
    return 0;
}
/* v50：抓最后一帧被拒/被忽视的现场（最多 64B），宿主机可原样重放。 */
static __attribute__((noinline)) void standalone_dns_capture(struct standalone_dns_diag *d, uint8_t kind,
                                          int status, const uint8_t *frame, size_t length)
{
    size_t i, n;
    if (d == (struct standalone_dns_diag *)0 || frame == (const uint8_t *)0) return;
    d->prob_kind = kind;
    d->prob_len = (uint16_t)(length > 0xFFFFu ? 0xFFFFu : length);
    d->prob_status = (int32_t)status;
    n = length < 64u ? length : 64u;
    for (i = 0u; i < 64u; ++i) d->prob_bytes[i] = (i < n) ? frame[i] : 0u;
}
/* v50：抓最后一帧被成功消费的现场（作为对照基线）。 */
static __attribute__((noinline)) void standalone_dns_capture_good(struct standalone_dns_diag *d, int status,
                                               const uint8_t *frame, size_t length)
{
    size_t i, n;
    if (d == (struct standalone_dns_diag *)0 || frame == (const uint8_t *)0) return;
    d->good_len = (uint16_t)(length > 0xFFFFu ? 0xFFFFu : length);
    d->good_status = (int32_t)status;
    n = length < 64u ? length : 64u;
    for (i = 0u; i < 64u; ++i) d->good_bytes[i] = (i < n) ? frame[i] : 0u;
}
/* v50：最朴素的形状探测（绕过 raw_udp_parse 的严格校验）。 */
static __attribute__((noinline)) void standalone_dns_probe(struct standalone_dns_diag *d,
                                        const uint8_t *frame, size_t length)
{
    if (d == (struct standalone_dns_diag *)0 || frame == (const uint8_t *)0) return;
    if (length >= 28u && frame[0] == 0x45u && frame[9] == 17u) {
        uint16_t sp = raw_udp_read16(frame + 20u);
        if (sp == 53u) ++d->look_dns; else ++d->look_udp;
    } else {
        ++d->look_other;
    }
}

struct standalone_dns {
    uint8_t source_ip[4];
    uint8_t dns_ip[4];
    uint16_t query_id;
    uint16_t sport;   /* ★v87：本次查询使用的源端口（手机 NetProxy 按四元组缓存会话）*/
    uint16_t last_transport_status;
    int status;
    uint8_t phase;
    uint8_t reserved[3];
    char hostname[DNS_CODEC_MAX_NAME + 2u];
    size_t tx_length;
    uint8_t tx_frame[STANDALONE_DNS_MAX_FRAME];
    struct dns_codec_result result;
};

static inline void standalone_dns_zero(void *p, size_t n)
{
    uint8_t *b = (uint8_t *)p;
    while (n != 0u) { *b++ = 0u; --n; }
}

static inline int standalone_dns_same_ip(const uint8_t *a, const uint8_t *b)
{
    return raw_udp_same_ip(a, b);
}

/* Starts one A transaction and builds a complete raw IPv4/UDP frame in tx_frame.
 * id is the DNS transaction ID chosen by the caller. A successful start puts
 * the state in WAIT_REPLY; start may be called again to replace a completed
 * or failed transaction. */
static inline int standalone_dns_start(struct standalone_dns *state,
                                       const char *hostname, uint16_t id,
                                       const uint8_t source_ip[4],
                                       const uint8_t dns_ip[4],
                                       uint16_t sport)
{
    uint8_t query[STANDALONE_DNS_MAX_QUERY];
    size_t query_length = 0u;
    size_t i;
    int rc;
    if (state == (struct standalone_dns *)0 || hostname == (const char *)0 ||
        source_ip == (const uint8_t *)0 || dns_ip == (const uint8_t *)0)
        return DNS_CODEC_BAD_ARGUMENT;
    standalone_dns_zero(state, sizeof(*state));
    state->query_id = id;
    state->phase = STANDALONE_DNS_IDLE;
    state->status = DNS_CODEC_BAD_ARGUMENT;
    rc = dns_codec_build_query(query, sizeof(query), id, hostname,
                               DNS_CODEC_TYPE_A, &query_length);
    if (rc != DNS_CODEC_OK) {
        state->status = rc;
        state->phase = STANDALONE_DNS_FAILED;
        return rc;
    }
    /* build_query has already bounded and validated this name. */
    for (i = 0u; i < sizeof(state->hostname); ++i) {
        state->hostname[i] = hostname[i];
        if (hostname[i] == '\0') break;
    }
    if (sport == 0u) sport = (uint16_t)STANDALONE_DNS_SOURCE_PORT;
    state->sport = sport;
    rc = raw_udp_build(state->tx_frame, sizeof(state->tx_frame), source_ip,
                       dns_ip, sport,
                       (uint16_t)STANDALONE_DNS_SERVER_PORT, id, query,
                       query_length, &state->tx_length);
    if (rc != RAW_UDP_OK) {
        state->status = rc;
        state->phase = STANDALONE_DNS_FAILED;
        return rc;
    }
    state->source_ip[0] = source_ip[0]; state->source_ip[1] = source_ip[1];
    state->source_ip[2] = source_ip[2]; state->source_ip[3] = source_ip[3];
    state->dns_ip[0] = dns_ip[0]; state->dns_ip[1] = dns_ip[1];
    state->dns_ip[2] = dns_ip[2]; state->dns_ip[3] = dns_ip[3];
    state->status = DNS_CODEC_OK;
    state->phase = STANDALONE_DNS_WAIT_REPLY;
    return DNS_CODEC_OK;
}

static inline const uint8_t *standalone_dns_tx(const struct standalone_dns *state,
                                               size_t *length)
{
    if (length != (size_t *)0) *length = 0u;
    if (state == (const struct standalone_dns *)0 ||
        state->phase != STANDALONE_DNS_WAIT_REPLY) return (const uint8_t *)0;
    if (length != (size_t *)0) *length = state->tx_length;
    return state->tx_frame;
}

/* Returns IGNORED for malformed/unrelated traffic, so the caller must not
 * drop a frame from any other flow. Returns CONSUMED only after a valid UDP
 * frame matches DNS server/client IPs, 53->53167 and the pending DNS ID. A
 * matching but malformed DNS payload returns FAILED_FRAME and transitions to
 * FAILED; transport parse errors remain IGNORED and expose raw status.
 * ★ v41：核心判据改为"最近 ID 环 + 问题名匹配"；IP/端口四项降级为统计
 * （手机侧改写会被记为 loose_accepts，逐项 ignored 计数归因）。 */
static inline int standalone_dns_consume(struct standalone_dns *state,
                                         const uint8_t *frame, size_t length,
                                         struct standalone_dns_diag *diag)
{
    struct raw_udp_view view;
    uint16_t pid;
    int rc;
    if (state == (struct standalone_dns *)0 ||
        state->phase != STANDALONE_DNS_WAIT_REPLY) {
        if (diag != (struct standalone_dns_diag *)0) ++diag->ign_idle;
        standalone_dns_probe(diag, frame, length);          /* ★v50：不在等待态的帧也看形状 */
        standalone_dns_capture(diag, 6u, 0, frame, length);
        return STANDALONE_DNS_IGNORED;
    }
    rc = raw_udp_parse(frame, length, &view);
    state->last_transport_status = (uint16_t)(rc < 0 ? -rc : rc);
    if (rc != RAW_UDP_OK) {
        if (diag != (struct standalone_dns_diag *)0) {
            ++diag->ign_parse;
            if (rc < 0 && -rc < 10) ++diag->parse_fail_hist[-rc];   /* ★v50：精确原因 */
        }
        standalone_dns_probe(diag, frame, length);          /* ★v50：坏帧也看形状 */
        standalone_dns_capture(diag, 1u, rc, frame, length);
        return STANDALONE_DNS_IGNORED;
    }
    if (diag != (struct standalone_dns_diag *)0) {
        diag->last_ign_sport = view.source_port;
        diag->last_ign_dport = view.destination_port;
        diag->last_ign_id = view.payload_length >= 2u ? raw_udp_read16(view.payload) : 0u;
        diag->last_ign_status = state->last_transport_status;
        for (rc = 0; rc < 4; ++rc) {
            diag->last_ign_sip[rc] = view.source_ip[rc];
            diag->last_ign_dip[rc] = view.destination_ip[rc];
        }
    }
    if (view.source_port != (uint16_t)STANDALONE_DNS_SERVER_PORT) {
        if (diag != (struct standalone_dns_diag *)0) ++diag->ign_port;
        standalone_dns_capture(diag, 2u, 0, frame, length);
        return STANDALONE_DNS_IGNORED;
    }
    if (view.payload_length < 2u) {
        if (diag != (struct standalone_dns_diag *)0) ++diag->ign_parse;
        standalone_dns_capture(diag, 1u, 0, frame, length);
        return STANDALONE_DNS_IGNORED;
    }
    pid = raw_udp_read16(view.payload);
    if (!standalone_dns_diag_recent(diag, pid)) {
        if (diag != (struct standalone_dns_diag *)0) ++diag->ign_id;
        standalone_dns_capture(diag, 3u, (int)pid, frame, length);
        return STANDALONE_DNS_IGNORED;
    }
    if (diag != (struct standalone_dns_diag *)0) {
        /* 原来的四项严格条件——只统计，不拒绝 */
        if (standalone_dns_same_ip(view.source_ip, state->dns_ip) &&
            standalone_dns_same_ip(view.destination_ip, state->source_ip) &&
            view.destination_port == (state->sport ? state->sport : (uint16_t)STANDALONE_DNS_SOURCE_PORT))
            ++diag->strict_ok;
        else ++diag->loose_accepts;
    }
    rc = dns_codec_parse_response(view.payload, view.payload_length, pid,
                                  state->hostname, DNS_CODEC_TYPE_A, &state->result);
    /* A different question/QR/opcode is not this transaction. */
    if (rc == DNS_CODEC_MISMATCH) {
        if (diag != (struct standalone_dns_diag *)0) ++diag->ign_qname;
        standalone_dns_capture(diag, 4u, rc, frame, length);
        return STANDALONE_DNS_IGNORED;
    }
    state->status = rc;
    if (rc != DNS_CODEC_OK) {
        state->phase = STANDALONE_DNS_FAILED;
        standalone_dns_capture(diag, 5u, rc, frame, length); /* ★v50：编解码拒绝的帧也留档 */
        return STANDALONE_DNS_FAILED_FRAME;
    }
    standalone_dns_capture_good(diag, rc, frame, length);    /* ★v50：对照基线 */
    state->phase = STANDALONE_DNS_DONE;
    return STANDALONE_DNS_CONSUMED;
}

/* ★ v52（socket 版）：只吃 DNS 负载 —— 走系统 socket 时内核已按四元组过滤，
 * IP/源端口两项天然成立（connected socket），无需再解析 IPv4/UDP 头。
 * 判据仍是两道闸：ID ∈ 最近 8 次查询环 + 问题名逐字节匹配（dns_codec_parse_response）。
 * 语义与 standalone_dns_consume 完全一致（IGNORED / CONSUMED / FAILED_FRAME），
 * 诊断归因也复用同一套计数器。 */
static inline int standalone_dns_consume_payload(struct standalone_dns *state,
                                                 const uint8_t *payload, size_t length,
                                                 struct standalone_dns_diag *diag)
{
    uint16_t pid;
    int rc;
    if (state == (struct standalone_dns *)0 || payload == (const uint8_t *)0) {
        if (diag != (struct standalone_dns_diag *)0) ++diag->ign_parse;
        return STANDALONE_DNS_IGNORED;
    }
    if (state->phase != STANDALONE_DNS_WAIT_REPLY) {
        if (diag != (struct standalone_dns_diag *)0) ++diag->ign_idle;
        return STANDALONE_DNS_IGNORED;
    }
    if (length < 2u) {
        if (diag != (struct standalone_dns_diag *)0) ++diag->ign_parse;
        return STANDALONE_DNS_IGNORED;
    }
    state->last_transport_status = 0u;
    pid = raw_udp_read16(payload);
    if (!standalone_dns_diag_recent(diag, pid)) {
        if (diag != (struct standalone_dns_diag *)0) ++diag->ign_id;
        standalone_dns_capture(diag, 3u, (int)pid, payload, length);
        return STANDALONE_DNS_IGNORED;
    }
    if (diag != (struct standalone_dns_diag *)0) ++diag->strict_ok; /* 连接态：四元组已由内核保证 */
    rc = dns_codec_parse_response(payload, length, pid,
                                  state->hostname, DNS_CODEC_TYPE_A, &state->result);
    /* ★ v63（宿主测试揪出）：ID 属于我们、但问题名/格式不符 ⇒ 这**不是我们的回包**
     *   （别人/缓存/畸形包都可能出现），只该记账并忽略，**绝不能把在飞查询打死**
     *   （那会让一次外来回包直接毁掉本次解析）。 */
    if (rc != DNS_CODEC_OK) {
        if (diag != (struct standalone_dns_diag *)0) {
            if (rc == DNS_CODEC_MISMATCH) ++diag->ign_qname; else ++diag->ign_parse;
        }
        standalone_dns_capture(diag, (rc == DNS_CODEC_MISMATCH) ? 4u : 5u, rc, payload, length);
        return STANDALONE_DNS_IGNORED;
    }
    state->status = rc;
    standalone_dns_capture_good(diag, rc, payload, length);
    state->phase = STANDALONE_DNS_DONE;
    return STANDALONE_DNS_CONSUMED;
}

/* Timeout/cancellation scheduling belongs to the caller. Any negative
 * caller status (e.g. -20 for timeout) may be recorded without doing I/O. */
static inline void standalone_dns_fail(struct standalone_dns *state, int status)
{
    if (state == (struct standalone_dns *)0 ||
        state->phase != STANDALONE_DNS_WAIT_REPLY) return;
    standalone_dns_zero(&state->result, sizeof(state->result));
    state->status = status < 0 ? status : DNS_CODEC_BAD_ARGUMENT;
    state->phase = STANDALONE_DNS_FAILED;
}

#endif
