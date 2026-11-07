#ifndef SYSPATCH_NAP_SDP_H
#define SYSPATCH_NAP_SDP_H
#include <stdint.h>
/* Independent SDP ServiceSearch client. All multibyte wire fields are BE.
 * No firmware addresses, allocations, or Bluetooth calls in this parser.
 * Results identify NAP service records, not a successful PAN connection. */
struct nap_sdp {
    uint16_t transaction,total;
    uint8_t continuation[16],continuation_size,rounds,done,count;
    uint32_t handles[16];
};
void nap_sdp_init(struct nap_sdp *);
int nap_sdp_request(struct nap_sdp *,uint8_t *out,unsigned capacity);
/* 1: complete, 0: continuation required, negative: invalid/error response. */
int nap_sdp_response(struct nap_sdp *,const uint8_t *,unsigned length);
#endif
