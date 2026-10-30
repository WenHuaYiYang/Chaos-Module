#ifndef RAW_UDP_H
#define RAW_UDP_H

#include <stddef.h>
#include <stdint.h>

/* Raw IPv4/UDP codec. Addresses are four bytes in network byte order.
 * No allocation, libc, globals, firmware calls, or I/O. Input is one exact
 * TUN IP frame (no link header or trailing link-layer padding). */
#define RAW_UDP_HEADER_SIZE 28u
#define RAW_UDP_MAX_PAYLOAD 65507u

enum raw_udp_status {
    RAW_UDP_OK = 0,
    RAW_UDP_BAD_ARGUMENT = -1,
    RAW_UDP_NO_SPACE = -2,
    RAW_UDP_BAD_IPV4 = -3,
    RAW_UDP_BAD_LENGTH = -4,
    RAW_UDP_FRAGMENT = -5,
    RAW_UDP_NOT_UDP = -6,
    RAW_UDP_IP_CHECKSUM = -7,
    RAW_UDP_UDP_CHECKSUM = -8
};

struct raw_udp_view {
    const uint8_t *source_ip;
    const uint8_t *destination_ip;
    const uint8_t *payload;
    size_t payload_length;
    uint16_t source_port;
    uint16_t destination_port;
    uint16_t ip_id;
    uint16_t udp_checksum;
    uint8_t ip_header_length;
};

static inline uint16_t raw_udp_read16(const uint8_t *p)
{
    return (uint16_t)(((uint16_t)p[0] << 8) | p[1]);
}

static inline void raw_udp_write16(uint8_t *p, uint16_t value)
{
    p[0] = (uint8_t)(value >> 8);
    p[1] = (uint8_t)value;
}

/* Individual chunks must end on a 16-bit boundary except the last chunk.
 * IPv4 header, pseudo-header parts, and UDP frame meet this requirement. */
static inline uint32_t raw_udp_checksum_add(uint32_t sum,
                                           const uint8_t *p, size_t n)
{
    while (n >= 2u) {
        sum += raw_udp_read16(p);
        p += 2;
        n -= 2u;
    }
    if (n != 0u) sum += (uint32_t)p[0] << 8;
    return sum;
}

static inline uint16_t raw_udp_checksum_finish(uint32_t sum)
{
    while ((sum >> 16) != 0u) sum = (sum & 0xffffu) + (sum >> 16);
    return (uint16_t)~sum;
}

static inline uint16_t raw_udp_checksum(const uint8_t *p, size_t n)
{
    return raw_udp_checksum_finish(raw_udp_checksum_add(0u, p, n));
}

static inline int raw_udp_same_ip(const uint8_t *a, const uint8_t *b)
{
    return a[0] == b[0] && a[1] == b[1] &&
           a[2] == b[2] && a[3] == b[3];
}

static inline uint16_t raw_udp_datagram_checksum(const uint8_t *source_ip,
                                                const uint8_t *destination_ip,
                                                const uint8_t *udp,
                                                size_t udp_length)
{
    uint32_t sum = raw_udp_checksum_add(0u, source_ip, 4u);
    sum = raw_udp_checksum_add(sum, destination_ip, 4u);
    sum += 17u; /* Pseudo-header zero byte and IPPROTO_UDP byte. */
    sum += (uint32_t)udp_length;
    return raw_udp_checksum_finish(raw_udp_checksum_add(sum, udp, udp_length));
}

/* Build an unfragmented IPv4 frame with DF, TTL 64 and a nonzero UDP
 * checksum. Payload/output must not overlap, unless payload==out+28.
 * written is zero on every failure. */
static inline int raw_udp_build(uint8_t *out, size_t capacity,
                                const uint8_t source_ip[4],
                                const uint8_t destination_ip[4],
                                uint16_t source_port,
                                uint16_t destination_port, uint16_t ip_id,
                                const uint8_t *payload, size_t payload_length,
                                size_t *written)
{
    size_t i, length;
    uint16_t udp_length, checksum;
    uint8_t addresses[8];
    if (written != (size_t *)0) *written = 0u;
    if (out == (uint8_t *)0 || source_ip == (const uint8_t *)0 ||
        destination_ip == (const uint8_t *)0 || written == (size_t *)0 ||
        (payload == (const uint8_t *)0 && payload_length != 0u))
        return RAW_UDP_BAD_ARGUMENT;
    if (payload_length > RAW_UDP_MAX_PAYLOAD) return RAW_UDP_BAD_LENGTH;
    length = RAW_UDP_HEADER_SIZE + payload_length;
    if (capacity < length) return RAW_UDP_NO_SPACE;
    /* Copy address arguments before writing out: pointers into the
     * output header are supported (e.g. reversing an existing packet). */
    for (i = 0u; i < 4u; ++i) {
        addresses[i] = source_ip[i];
        addresses[i + 4u] = destination_ip[i];
    }
    for (i = 0u; i < 8u; ++i) out[12u + i] = addresses[i];
    for (i = 0u; i < 12u; ++i) out[i] = 0u;
    out[0] = 0x45u;
    raw_udp_write16(out + 2u, (uint16_t)length);
    raw_udp_write16(out + 4u, ip_id);
    raw_udp_write16(out + 6u, 0x4000u);
    out[8] = 64u;
    out[9] = 17u;
    raw_udp_write16(out + 10u, raw_udp_checksum(out, 20u));
    udp_length = (uint16_t)(8u + payload_length);
    raw_udp_write16(out + 20u, source_port);
    raw_udp_write16(out + 22u, destination_port);
    raw_udp_write16(out + 24u, udp_length);
    raw_udp_write16(out + 26u, 0u);
    for (i = 0u; i < payload_length; ++i) out[28u + i] = payload[i];
    checksum = raw_udp_datagram_checksum(out + 12u, out + 16u,
                                         out + 20u, udp_length);
    /* RFC 768: calculated zero is transmitted as all ones. */
    raw_udp_write16(out + 26u, checksum == 0u ? 0xffffu : checksum);
    *written = length;
    return RAW_UDP_OK;
}

/* IPv4 options are included in the header checksum and IHL bounds check.
 * Their content is not interpreted. Reject all fragmented/reserved-flag
 * packets. UDP checksum=0 is valid only because this is IPv4. On failure
 * no pointers remain in view; zero-length UDP payloads are accepted. */
static inline int raw_udp_parse(const uint8_t *frame, size_t length,
                                struct raw_udp_view *view)
{
    size_t ihl, udp_length;
    uint16_t fragment, checksum;
    uint8_t *clear;
    size_t i;
    if (view == (struct raw_udp_view *)0) return RAW_UDP_BAD_ARGUMENT;
    clear = (uint8_t *)view;
    for (i = 0u; i < sizeof(*view); ++i) clear[i] = 0u;
    if (frame == (const uint8_t *)0) return RAW_UDP_BAD_ARGUMENT;
    if (length < 20u || (frame[0] >> 4) != 4u)
        return RAW_UDP_BAD_IPV4;
    ihl = (size_t)(frame[0] & 15u) * 4u;
    if (ihl < 20u || ihl > length) return RAW_UDP_BAD_IPV4;
    if ((size_t)raw_udp_read16(frame + 2u) != length ||
        length < ihl + 8u) return RAW_UDP_BAD_LENGTH;
    fragment = raw_udp_read16(frame + 6u);
    if ((fragment & 0xbfffu) != 0u) return RAW_UDP_FRAGMENT;
    if (frame[9] != 17u) return RAW_UDP_NOT_UDP;
    if (raw_udp_checksum(frame, ihl) != 0u) return RAW_UDP_IP_CHECKSUM;
    udp_length = raw_udp_read16(frame + ihl + 4u);
    if (udp_length < 8u || udp_length != length - ihl)
        return RAW_UDP_BAD_LENGTH;
    checksum = raw_udp_read16(frame + ihl + 6u);
    if (checksum != 0u &&
        raw_udp_datagram_checksum(frame + 12u, frame + 16u,
                                  frame + ihl, udp_length) != 0u)
        return RAW_UDP_UDP_CHECKSUM;
    view->source_ip = frame + 12u;
    view->destination_ip = frame + 16u;
    view->source_port = raw_udp_read16(frame + ihl);
    view->destination_port = raw_udp_read16(frame + ihl + 2u);
    view->ip_id = raw_udp_read16(frame + 4u);
    view->udp_checksum = checksum;
    view->ip_header_length = (uint8_t)ihl;
    view->payload = frame + ihl + 8u;
    view->payload_length = udp_length - 8u;
    return RAW_UDP_OK;
}

#endif
