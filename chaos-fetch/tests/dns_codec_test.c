#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "dns_codec.h"

static void put16(uint8_t *p, uint16_t v)
{
    p[0] = (uint8_t)(v >> 8);
    p[1] = (uint8_t)v;
}

static void put32(uint8_t *p, uint32_t v)
{
    p[0] = (uint8_t)(v >> 24);
    p[1] = (uint8_t)(v >> 16);
    p[2] = (uint8_t)(v >> 8);
    p[3] = (uint8_t)v;
}

static size_t make_response(const char *name, uint16_t qtype, uint8_t *out, size_t cap)
{
    size_t n = 0;
    assert(dns_codec_build_query(out, cap, 0x1234, name, qtype, &n) == DNS_CODEC_OK);
    out[2] = 0x81;
    out[3] = 0x80;
    return n;
}

static size_t add_a(uint8_t *p, const uint8_t ip[4], uint32_t ttl)
{
    p[0] = 0xc0; p[1] = 0x0c;
    put16(p + 2, DNS_CODEC_TYPE_A);
    put16(p + 4, 1);
    put32(p + 6, ttl);
    put16(p + 10, 4);
    memcpy(p + 12, ip, 4);
    return 16;
}

static void test_query(void)
{
    uint8_t q[512];
    size_t n = 0;
    assert(dns_codec_build_query(q, sizeof(q), 0xabcd, "Example.COM.",
                                 DNS_CODEC_TYPE_A, &n) == DNS_CODEC_OK);
    assert(n == 29);
    assert(q[0] == 0xab && q[1] == 0xcd && q[2] == 1 && q[3] == 0);
    assert(memcmp(q + 12, "\7example\3com\0\0\1\0\1", 17) == 0);
    assert(dns_codec_build_query(q, 10, 1, "a.com", 1, &n) == DNS_CODEC_NO_SPACE);
    assert(n == 0);
    assert(dns_codec_build_query(q, sizeof(q), 1, "a..com", 1, &n) == DNS_CODEC_BAD_ARGUMENT);
    assert(dns_codec_build_query(q, sizeof(q), 1, "bad!.com", 1, &n) == DNS_CODEC_BAD_ARGUMENT);
}

static void test_a_response(void)
{
    uint8_t p[512], ip[4] = {1, 2, 3, 4};
    size_t n = make_response("Example.COM", DNS_CODEC_TYPE_A, p, sizeof(p));
    struct dns_codec_result r;
    put16(p + 6, 1);
    n += add_a(p + n, ip, 60);
    assert(dns_codec_parse_response(p, n, 0x1234, "example.com.",
                                    DNS_CODEC_TYPE_A, &r) == DNS_CODEC_OK);
    assert(r.address_count == 1 && memcmp(r.addresses[0].bytes, ip, 4) == 0);
    assert(r.addresses[0].ttl == 60 && strcmp(r.canonical_name, "example.com") == 0);
    assert(dns_codec_parse_response(p, n, 0x1235, "example.com",
                                    DNS_CODEC_TYPE_A, &r) == DNS_CODEC_MISMATCH);
    assert(r.address_count == 0);
}

static void test_cname_response(void)
{
    static const uint8_t target[] = {4, 'h', 'o', 's', 't', 7, 'e', 'x', 'a', 'm', 'p', 'l', 'e', 0};
    uint8_t p[512], ip[4] = {5, 6, 7, 8};
    size_t n = make_response("alias.example", DNS_CODEC_TYPE_A, p, sizeof(p));
    size_t rr, target_offset;
    struct dns_codec_result r;
    put16(p + 6, 2);
    rr = n;
    p[rr] = 0xc0; p[rr + 1] = 0x0c;
    put16(p + rr + 2, 5);
    put16(p + rr + 4, 1);
    put32(p + rr + 6, 30);
    put16(p + rr + 10, sizeof(target));
    target_offset = rr + 12;
    memcpy(p + target_offset, target, sizeof(target));
    n = target_offset + sizeof(target);
    p[n] = (uint8_t)(0xc0 | (target_offset >> 8));
    p[n + 1] = (uint8_t)target_offset;
    put16(p + n + 2, DNS_CODEC_TYPE_A);
    put16(p + n + 4, 1);
    put32(p + n + 6, 45);
    put16(p + n + 10, 4);
    memcpy(p + n + 12, ip, 4);
    n += 16;
    assert(dns_codec_parse_response(p, n, 0x1234, "alias.example",
                                    DNS_CODEC_TYPE_A, &r) == DNS_CODEC_OK);
    assert(r.cname_count == 1 && r.address_count == 1);
    assert(strcmp(r.canonical_name, "host.example") == 0);
    assert(r.addresses[0].ttl == 30);
}

static void test_errors_and_fuzz(void)
{
    uint8_t p[512], ip[4] = {9, 9, 9, 9};
    size_t n = make_response("example.com", DNS_CODEC_TYPE_A, p, sizeof(p));
    struct dns_codec_result r;
    uint32_t seed = 0x12345678;
    unsigned i;
    put16(p + 6, 1);
    n += add_a(p + n, ip, 60);
    p[3] = 0x83;
    assert(dns_codec_parse_response(p, n, 0x1234, "example.com", 1, &r) == DNS_CODEC_RCODE);
    assert(r.rcode == 3 && r.address_count == 0);
    p[3] = 0x80;
    p[n - 16] = 0xc0;
    p[n - 15] = (uint8_t)(n - 16);
    assert(dns_codec_parse_response(p, n, 0x1234, "example.com", 1, &r) == DNS_CODEC_MALFORMED);

    for (i = 0; i < 50000; ++i) {
        size_t j, len;
        seed = seed * 1664525u + 1013904223u;
        len = seed & 255u;
        for (j = 0; j < len; ++j) {
            seed = seed * 1664525u + 1013904223u;
            p[j] = (uint8_t)(seed >> 24);
        }
        (void)dns_codec_parse_response(p, len, 0x1234, "example.com", 1, &r);
    }
}

int main(void)
{
    test_query();
    test_a_response();
    test_cname_response();
    test_errors_and_fuzz();
    puts("dns_codec tests passed");
    return 0;
}
