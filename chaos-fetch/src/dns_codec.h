#ifndef DNS_CODEC_H
#define DNS_CODEC_H

#include <stddef.h>
#include <stdint.h>

/* Standalone DNS wire codec. No allocation, libc calls, I/O, or global state. */
#define DNS_CODEC_TYPE_A 1u
#define DNS_CODEC_TYPE_AAAA 28u
#define DNS_CODEC_MAX_NAME 253u
#define DNS_CODEC_MAX_PACKET 4096u
#define DNS_CODEC_MAX_RECORDS 64u
#define DNS_CODEC_MAX_CNAME 8u
#define DNS_CODEC_MAX_POINTERS 16u
#define DNS_CODEC_MAX_ADDRESSES 4u
#define DNS_CODEC_RESULT_MORE_ADDRESSES 1u

enum dns_codec_status {
    DNS_CODEC_OK = 0,
    DNS_CODEC_BAD_ARGUMENT = -1,
    DNS_CODEC_NO_SPACE = -2,
    DNS_CODEC_MALFORMED = -3,
    DNS_CODEC_MISMATCH = -4,
    DNS_CODEC_TRUNCATED = -5,
    DNS_CODEC_RCODE = -6,
    DNS_CODEC_NO_ADDRESS = -7,
    DNS_CODEC_LIMIT = -8
};

struct dns_codec_address {
    /* Network byte order. A uses bytes[0..3]; AAAA uses all 16 bytes. */
    uint8_t bytes[16];
    uint32_t ttl;
};

struct dns_codec_result {
    uint16_t qtype;
    uint16_t rcode; /* Includes EDNS extended RCODE, when present. */
    uint8_t address_count;
    uint8_t cname_count;
    uint8_t flags;
    uint8_t reserved;
    char canonical_name[DNS_CODEC_MAX_NAME + 1u];
    struct dns_codec_address addresses[DNS_CODEC_MAX_ADDRESSES];
};

/* ASCII host names; case insensitive; one terminal dot is accepted.
 * Labels contain letters, digits, '-' or '_', are 1..63 bytes, and the
 * complete name is 1..253 bytes. Unicode must be converted to punycode by
 * the caller. Emits one IN question with RD set. written is 0 on failure. */
int dns_codec_build_query(uint8_t *out, size_t capacity, uint16_t id,
                          const char *hostname, uint16_t qtype,
                          size_t *written);

/* Validate a complete DNS payload (not IP/UDP headers) against one query.
 * QR, opcode, ID, IN question, reserved Z bit, TC, RCODE, all RR lengths,
 * compressed names, and bounded CNAME chains are checked. Only matching
 * answer-section A/AAAA records are returned; unrelated/additional data
 * cannot satisfy the query. Result addresses/count are cleared on failure.
 * canonical_name is lowercase without a terminal dot. Its value on error
 * is diagnostic only. TTL is the minimum along the accepted CNAME chain.
 * At most four unique addresses are returned; MORE_ADDRESSES flags extras.
 * Wire compression pointers must refer backwards (RFC 1035 prior names). */
int dns_codec_parse_response(const uint8_t *packet, size_t length,
                             uint16_t expected_id,
                             const char *expected_name,
                             uint16_t expected_qtype,
                             struct dns_codec_result *result);

#endif
