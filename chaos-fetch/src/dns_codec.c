#include "dns_codec.h"

struct dns_rr {
    uint16_t type;
    uint16_t class_code;
    uint16_t data_length;
    uint32_t ttl;
    size_t data_offset;
};

static uint16_t read16(const uint8_t *p)
{
    return (uint16_t)(((uint16_t)p[0] << 8) | p[1]);
}

static uint32_t read32(const uint8_t *p)
{
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) |
           ((uint32_t)p[2] << 8) | p[3];
}

static void write16(uint8_t *p, uint16_t v)
{
    p[0] = (uint8_t)(v >> 8);
    p[1] = (uint8_t)v;
}

static int host_character(unsigned char c)
{
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
           (c >= '0' && c <= '9') || c == '-' || c == '_';
}

static char lower_ascii(unsigned char c)
{
    return (char)((c >= 'A' && c <= 'Z') ? (c + ('a' - 'A')) : c);
}

static void clear_bytes(void *dst, size_t n)
{
    uint8_t *p = (uint8_t *)dst;
    while (n != 0u) { *p++ = 0u; --n; }
}

static void copy_name(char *dst, const char *src)
{
    do { *dst++ = *src; } while (*src++ != '\0');
}

static int same_name(const char *a, const char *b)
{
    while (*a != '\0' && *a == *b) { ++a; ++b; }
    return *a == *b;
}

static int normalize_name(const char *src, char *dst, size_t *name_length)
{
    size_t n = 0u, label = 0u, i;
    if (src == (const char *)0) return DNS_CODEC_BAD_ARGUMENT;
    /* The bounded read includes room for a terminal dot and NUL. */
    while (n <= DNS_CODEC_MAX_NAME + 1u && src[n] != '\0') ++n;
    if (n == 0u || n > DNS_CODEC_MAX_NAME + 1u) return DNS_CODEC_BAD_ARGUMENT;
    if (src[n - 1u] == '.') --n;
    if (n == 0u || n > DNS_CODEC_MAX_NAME) return DNS_CODEC_BAD_ARGUMENT;
    for (i = 0u; i < n; ++i) {
        unsigned char c = (unsigned char)src[i];
        if (c == '.') {
            if (label == 0u) return DNS_CODEC_BAD_ARGUMENT;
            label = 0u;
        } else {
            if (!host_character(c) || ++label > 63u) return DNS_CODEC_BAD_ARGUMENT;
        }
        dst[i] = lower_ascii(c);
    }
    if (label == 0u) return DNS_CODEC_BAD_ARGUMENT;
    dst[n] = '\0';
    if (name_length != (size_t *)0) *name_length = n;
    return DNS_CODEC_OK;
}

/* Return the number of bytes consumed at the original wire position, not
 * the length reached through compression pointers. No recursion. */
static int decode_name(const uint8_t *packet, size_t length, size_t start,
                       char *out, size_t *consumed)
{
    size_t pos = start, written = 0u, used = 0u;
    size_t seen[DNS_CODEC_MAX_POINTERS];
    unsigned int jumps = 0u;
    int jumped = 0;
    if (start >= length) return DNS_CODEC_MALFORMED;
    for (;;) {
        uint8_t tag;
        size_t i;
        if (pos >= length) return DNS_CODEC_MALFORMED;
        tag = packet[pos];
        if ((tag & 0xc0u) == 0xc0u) {
            size_t target;
            if (length - pos < 2u) return DNS_CODEC_MALFORMED;
            target = ((size_t)(tag & 0x3fu) << 8) | packet[pos + 1u];
            /* Forward/self pointers are invalid and may form cycles. */
            if (target >= pos || target >= length) return DNS_CODEC_MALFORMED;
            if (jumps >= DNS_CODEC_MAX_POINTERS) return DNS_CODEC_LIMIT;
            for (i = 0u; i < jumps; ++i)
                if (seen[i] == target) return DNS_CODEC_MALFORMED;
            seen[jumps++] = target;
            if (!jumped) used += 2u;
            jumped = 1;
            pos = target;
            continue;
        }
        if ((tag & 0xc0u) != 0u) return DNS_CODEC_MALFORMED;
        ++pos;
        if (!jumped) ++used;
        if (tag == 0u) {
            out[written] = '\0';
            *consumed = used;
            return DNS_CODEC_OK;
        }
        if (length - pos < tag) return DNS_CODEC_MALFORMED;
        if (written != 0u) {
            if (written >= DNS_CODEC_MAX_NAME) return DNS_CODEC_MALFORMED;
            out[written++] = '.';
        }
        if ((size_t)tag > DNS_CODEC_MAX_NAME - written) return DNS_CODEC_MALFORMED;
        for (i = 0u; i < tag; ++i) {
            unsigned char c = packet[pos + i];
            if (!host_character(c)) return DNS_CODEC_MALFORMED;
            out[written++] = lower_ascii(c);
        }
        pos += tag;
        if (!jumped) used += tag;
    }
}

static int read_rr(const uint8_t *packet, size_t length, size_t *position,
                   char *owner, struct dns_rr *rr)
{
    size_t used, pos = *position;
    int rc = decode_name(packet, length, pos, owner, &used);
    if (rc != DNS_CODEC_OK) return rc;
    pos += used;
    if (pos > length || length - pos < 10u) return DNS_CODEC_MALFORMED;
    rr->type = read16(packet + pos);
    rr->class_code = read16(packet + pos + 2u);
    rr->ttl = read32(packet + pos + 4u);
    rr->data_length = read16(packet + pos + 8u);
    rr->data_offset = pos + 10u;
    if (length - rr->data_offset < rr->data_length) return DNS_CODEC_MALFORMED;
    *position = rr->data_offset + rr->data_length;
    return DNS_CODEC_OK;
}

static int read_cname(const uint8_t *packet, size_t length,
                      const struct dns_rr *rr, char *name)
{
    size_t used;
    int rc = decode_name(packet, length, rr->data_offset, name, &used);
    if (rc != DNS_CODEC_OK) return rc;
    if (used != rr->data_length || name[0] == '\0') return DNS_CODEC_MALFORMED;
    return DNS_CODEC_OK;
}

int dns_codec_build_query(uint8_t *out, size_t capacity, uint16_t id,
                          const char *hostname, uint16_t qtype,
                          size_t *written)
{
    char name[DNS_CODEC_MAX_NAME + 1u];
    size_t n, i = 0u, pos = 12u;
    int rc;
    if (written != (size_t *)0) *written = 0u;
    if (out == (uint8_t *)0 || written == (size_t *)0 ||
        (qtype != DNS_CODEC_TYPE_A && qtype != DNS_CODEC_TYPE_AAAA))
        return DNS_CODEC_BAD_ARGUMENT;
    rc = normalize_name(hostname, name, &n);
    if (rc != DNS_CODEC_OK) return rc;
    if (capacity < n + 18u) return DNS_CODEC_NO_SPACE;
    clear_bytes(out, 12u);
    write16(out, id);
    write16(out + 2u, 0x0100u);
    write16(out + 4u, 1u);
    while (i < n) {
        size_t label_start = i, j;
        while (i < n && name[i] != '.') ++i;
        out[pos++] = (uint8_t)(i - label_start);
        for (j = label_start; j < i; ++j) out[pos++] = (uint8_t)name[j];
        if (i < n) ++i;
    }
    out[pos++] = 0u;
    write16(out + pos, qtype); pos += 2u;
    write16(out + pos, 1u); pos += 2u;
    *written = pos;
    return DNS_CODEC_OK;
}

static void add_address(struct dns_codec_result *result, const uint8_t *bytes,
                        size_t n, uint32_t ttl)
{
    unsigned int i;
    size_t j;
    for (i = 0u; i < result->address_count; ++i) {
        for (j = 0u; j < n && result->addresses[i].bytes[j] == bytes[j]; ++j) {}
        if (j == n) {
            if (ttl < result->addresses[i].ttl) result->addresses[i].ttl = ttl;
            return;
        }
    }
    if (result->address_count == DNS_CODEC_MAX_ADDRESSES) {
        result->flags |= DNS_CODEC_RESULT_MORE_ADDRESSES;
        return;
    }
    i = result->address_count++;
    for (j = 0u; j < n; ++j) result->addresses[i].bytes[j] = bytes[j];
    result->addresses[i].ttl = ttl;
}

static int parse_inner(const uint8_t *packet, size_t length,
                       uint16_t expected_id, const char *expected_name,
                       uint16_t expected_qtype, struct dns_codec_result *result)
{
    char expected[DNS_CODEC_MAX_NAME + 1u];
    char owner[DNS_CODEC_MAX_NAME + 1u];
    char target[DNS_CODEC_MAX_NAME + 1u];
    char next[DNS_CODEC_MAX_NAME + 1u];
    uint16_t flags, answers, authorities, additionals;
    uint16_t cname_records[DNS_CODEC_MAX_CNAME];
    size_t position, used, answer_start, count, i;
    uint32_t chain_ttl = 0xffffffffu;
    unsigned int depth = 0u, opt_seen = 0u;
    int rc;
    if (packet == (const uint8_t *)0 ||
        (expected_qtype != DNS_CODEC_TYPE_A && expected_qtype != DNS_CODEC_TYPE_AAAA))
        return DNS_CODEC_BAD_ARGUMENT;
    rc = normalize_name(expected_name, expected, (size_t *)0);
    if (rc != DNS_CODEC_OK) return rc;
    if (length > DNS_CODEC_MAX_PACKET) return DNS_CODEC_LIMIT;
    if (length < 12u) return DNS_CODEC_MALFORMED;
    flags = read16(packet + 2u);
    if (read16(packet) != expected_id || (flags & 0x8000u) == 0u ||
        (flags & 0x7800u) != 0u) return DNS_CODEC_MISMATCH;
    if ((flags & 0x0040u) != 0u) return DNS_CODEC_MALFORMED;
    if (read16(packet + 4u) != 1u) return DNS_CODEC_MISMATCH;
    result->qtype = expected_qtype;
    result->rcode = (uint16_t)(flags & 0x000fu);
    position = 12u;
    rc = decode_name(packet, length, position, owner, &used);
    if (rc != DNS_CODEC_OK) return rc;
    position += used;
    if (length - position < 4u) return DNS_CODEC_MALFORMED;
    if (!same_name(expected, owner) || read16(packet + position) != expected_qtype ||
        read16(packet + position + 2u) != 1u) return DNS_CODEC_MISMATCH;
    position += 4u;
    if ((flags & 0x0200u) != 0u) return DNS_CODEC_TRUNCATED;
    if (result->rcode != 0u) return DNS_CODEC_RCODE;
    answers = read16(packet + 6u);
    authorities = read16(packet + 8u);
    additionals = read16(packet + 10u);
    count = (size_t)answers + authorities + additionals;
    if (count > DNS_CODEC_MAX_RECORDS) return DNS_CODEC_LIMIT;
    answer_start = position;
    /* Validate the complete payload before accepting any address. */
    for (i = 0u; i < count; ++i) {
        struct dns_rr rr;
        rc = read_rr(packet, length, &position, owner, &rr);
        if (rc != DNS_CODEC_OK) return rc;
        if (rr.class_code == 1u) {
            if ((rr.type == DNS_CODEC_TYPE_A && rr.data_length != 4u) ||
                (rr.type == DNS_CODEC_TYPE_AAAA && rr.data_length != 16u))
                return DNS_CODEC_MALFORMED;
            if (rr.type == 5u) {
                rc = read_cname(packet, length, &rr, target);
                if (rc != DNS_CODEC_OK) return rc;
            }
        }
        if (rr.type == 41u) {
            if (i < (size_t)answers + authorities || owner[0] != '\0' ||
                opt_seen++ != 0u) return DNS_CODEC_MALFORMED;
            result->rcode |= (uint16_t)((rr.ttl >> 24) << 4);
            if ((rr.ttl & 0x00ff0000u) != 0u) return DNS_CODEC_MALFORMED;
        }
    }
    if (position != length) return DNS_CODEC_MALFORMED;
    if (result->rcode != 0u) return DNS_CODEC_RCODE;
    copy_name(result->canonical_name, expected);
    for (;;) {
        int have_cname = 0, have_address = 0;
        uint16_t cname_index = 0u;
        uint32_t cname_ttl = 0xffffffffu;
        position = answer_start;
        next[0] = '\0';
        for (i = 0u; i < answers; ++i) {
            struct dns_rr rr;
            rc = read_rr(packet, length, &position, owner, &rr);
            if (rc != DNS_CODEC_OK) return rc;
            if (rr.class_code != 1u || !same_name(owner, result->canonical_name)) continue;
            if (rr.type == 5u) {
                rc = read_cname(packet, length, &rr, target);
                if (rc != DNS_CODEC_OK) return rc;
                if (have_cname && !same_name(next, target)) return DNS_CODEC_MALFORMED;
                if (!have_cname) { copy_name(next, target); cname_index = (uint16_t)i; }
                have_cname = 1;
                if (rr.ttl < cname_ttl) cname_ttl = rr.ttl;
            } else if (rr.type == expected_qtype) {
                uint32_t ttl = rr.ttl < chain_ttl ? rr.ttl : chain_ttl;
                have_address = 1;
                add_address(result, packet + rr.data_offset, rr.data_length, ttl);
            }
        }
        if (have_cname && have_address) return DNS_CODEC_MALFORMED;
        if (!have_cname) return have_address ? DNS_CODEC_OK : DNS_CODEC_NO_ADDRESS;
        if (depth >= DNS_CODEC_MAX_CNAME) return DNS_CODEC_LIMIT;
        for (i = 0u; i < depth; ++i)
            if (cname_records[i] == cname_index) return DNS_CODEC_MALFORMED;
        cname_records[depth++] = cname_index;
        result->cname_count = (uint8_t)depth;
        if (cname_ttl < chain_ttl) chain_ttl = cname_ttl;
        copy_name(result->canonical_name, next);
    }
}

int dns_codec_parse_response(const uint8_t *packet, size_t length,
                             uint16_t expected_id, const char *expected_name,
                             uint16_t expected_qtype,
                             struct dns_codec_result *result)
{
    int rc;
    if (result == (struct dns_codec_result *)0) return DNS_CODEC_BAD_ARGUMENT;
    clear_bytes(result, sizeof(*result));
    rc = parse_inner(packet, length, expected_id, expected_name, expected_qtype, result);
    if (rc != DNS_CODEC_OK) {
        result->address_count = 0u;
        result->flags = 0u;
        clear_bytes(result->addresses, sizeof(result->addresses));
    }
    return rc;
}
