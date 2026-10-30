#ifndef DHCP_CODEC_H
#define DHCP_CODEC_H

#include <stddef.h>
#include <stdint.h>

/* DHCP payload codec only: no IP/UDP header, checksums, I/O, allocation,
 * libc calls or global state. All addresses are network-order byte arrays.
 * The caller owns retries, selected-server checks and the lease lifecycle. */
#define DHCP_CODEC_DISCOVER 1u
#define DHCP_CODEC_OFFER 2u
#define DHCP_CODEC_REQUEST 3u
#define DHCP_CODEC_ACK 5u
#define DHCP_CODEC_NAK 6u
#define DHCP_CODEC_MIN_CLIENT_PAYLOAD 300u
#define DHCP_CODEC_MAX_PAYLOAD 1472u
#define DHCP_CODEC_HAVE_MASK 1u
#define DHCP_CODEC_HAVE_ROUTER 2u
#define DHCP_CODEC_HAVE_DNS 4u
#define DHCP_CODEC_HAVE_LEASE 8u
#define DHCP_CODEC_HAVE_T1 16u
#define DHCP_CODEC_HAVE_T2 32u

enum dhcp_codec_status {
  DHCP_CODEC_OK=0, DHCP_CODEC_BAD_ARGUMENT=-1, DHCP_CODEC_NO_SPACE=-2,
  DHCP_CODEC_MALFORMED=-3, DHCP_CODEC_MISMATCH=-4,
  DHCP_CODEC_UNSUPPORTED=-5, DHCP_CODEC_LIMIT=-6
};
struct dhcp_codec_lease {
  uint8_t message_type;
  uint8_t present;
  uint8_t reserved[2];
  uint8_t address[4],server[4],mask[4],router[4],dns[4];
  /* ★ v71：option 6 (DNS) 允许 n%4==0 且 n>4 —— 真机/企业网常见两条 DNS。
   * 旧版只拷前 4 字节，第 2 条被静默丢弃 ⇒ 重试全打第 1 条、无 failover。
   * dns2_present 单独标记（present 位的 8 个 bit 已占满）。*/
  uint8_t dns2[4];
  uint8_t dns2_present;
  uint8_t reserved2[3];
  uint32_t lease_seconds,renew_seconds,rebind_seconds;
};

static inline uint32_t dhcp_codec_read32(const uint8_t *p) {
  return ((uint32_t)p[0]<<24)|((uint32_t)p[1]<<16)|((uint32_t)p[2]<<8)|p[3];
}
static inline void dhcp_codec_write32(uint8_t *p,uint32_t v) {
  p[0]=(uint8_t)(v>>24);p[1]=(uint8_t)(v>>16);p[2]=(uint8_t)(v>>8);p[3]=(uint8_t)v;
}
static inline void dhcp_codec_clear(void *p,size_t n) {
  uint8_t *b=(uint8_t *)p;while(n--) *b++=0;
}
static inline void dhcp_codec_copy(uint8_t *d,const uint8_t *s,size_t n) {
  while(n--) *d++=*s++;
}
static inline int dhcp_codec_same(const uint8_t *a,const uint8_t *b,size_t n) {
  while(n--) { if(*a++!=*b++) return 0; }
  return 1;
}
static inline int dhcp_codec_mac_valid(const uint8_t *mac) {
  unsigned i,any=0;if(!mac || (mac[0]&1u)) return 0;
  for(i=0;i<6u;++i) any|=mac[i];
  return any!=0;
}
/* Reject unspecified, loopback, multicast and reserved/broadcast addresses. */
static inline int dhcp_codec_ip_valid(const uint8_t *ip) {
  return ip && ip[0]!=0u && ip[0]!=127u && ip[0]<224u;
}
static inline int dhcp_codec_mask_valid(const uint8_t *ip) {
  uint32_t m=dhcp_codec_read32(ip),host=~m;
  return m!=0u && (host&(host+1u))==0u;
}

/* Build initial DISCOVER or selecting-state REQUEST. REQUEST always includes
 * requested-address (50) and server-id (54); renewal is deliberately outside
 * this codec's scope. The output is padded to RFC 2131's 300-byte client
 * minimum. Build arguments are checked before any output is changed. */
static inline int dhcp_codec_build(uint8_t *out,size_t capacity,uint32_t xid,
                                   const uint8_t chaddr[6],uint8_t type,
                                   const uint8_t requested[4],
                                   const uint8_t server[4],size_t *written) {
  size_t pos=240u;unsigned i;
  if(written) *written=0;
  if(!out || !written || !xid || !dhcp_codec_mac_valid(chaddr) ||
     (type!=DHCP_CODEC_DISCOVER && type!=DHCP_CODEC_REQUEST))
    return DHCP_CODEC_BAD_ARGUMENT;
  if(type==DHCP_CODEC_REQUEST &&
     (!dhcp_codec_ip_valid(requested) || !dhcp_codec_ip_valid(server)))
    return DHCP_CODEC_BAD_ARGUMENT;
  if(capacity<DHCP_CODEC_MIN_CLIENT_PAYLOAD) return DHCP_CODEC_NO_SPACE;
  dhcp_codec_clear(out,DHCP_CODEC_MIN_CLIENT_PAYLOAD);
  out[0]=1;out[1]=1;out[2]=6;
  dhcp_codec_write32(out+4,xid);out[10]=0x80;
  dhcp_codec_copy(out+28,chaddr,6);
  out[236]=99;out[237]=130;out[238]=83;out[239]=99;
  out[pos++]=53;out[pos++]=1;out[pos++]=type;
  out[pos++]=61;out[pos++]=7;out[pos++]=1;
  for(i=0;i<6u;++i) out[pos++]=chaddr[i];
  if(type==DHCP_CODEC_REQUEST) {
    out[pos++]=50;out[pos++]=4;dhcp_codec_copy(out+pos,requested,4);pos+=4;
    out[pos++]=54;out[pos++]=4;dhcp_codec_copy(out+pos,server,4);pos+=4;
  }
  out[pos++]=55;out[pos++]=7;
  out[pos++]=1;out[pos++]=3;out[pos++]=6;out[pos++]=51;
  out[pos++]=54;out[pos++]=58;out[pos++]=59;
  out[pos++]=57;out[pos++]=2;out[pos++]=2;out[pos++]=64; /* 576 bytes */
  out[pos]=255;*written=DHCP_CODEC_MIN_CLIENT_PAYLOAD;return DHCP_CODEC_OK;
}

/* Parse OFFER/ACK/NAK payloads. Every TLV is bounds-checked. Repeated recognized
 * options are rejected rather than concatenated; option-overload (52) is
 * unsupported, so file/sname cannot sneak in a second configuration. Missing
 * optional fields remain zero with their present bit clear. On every failure
 * the whole result is cleared. A NAK has a valid server-id but no lease data.
 * For ACK, callers must also compare server/address with the selected OFFER.
 */
static inline int dhcp_codec_parse(const uint8_t *packet,size_t length,
                                   uint32_t xid,const uint8_t chaddr[6],
                                   struct dhcp_codec_lease *result) {
  struct dhcp_codec_lease tmp;size_t pos=240u;unsigned seen=0,ended=0;
  int rc=DHCP_CODEC_MALFORMED;
  if(!result) return DHCP_CODEC_BAD_ARGUMENT;
  dhcp_codec_clear(result,sizeof(*result));dhcp_codec_clear(&tmp,sizeof(tmp));
  if(!packet || !xid || !dhcp_codec_mac_valid(chaddr)) return DHCP_CODEC_BAD_ARGUMENT;
  if(length>DHCP_CODEC_MAX_PAYLOAD) return DHCP_CODEC_LIMIT;
  if(length<241u) return DHCP_CODEC_MALFORMED;
  if(packet[0]!=2u || packet[1]!=1u || packet[2]!=6u ||
     dhcp_codec_read32(packet+4)!=xid || !dhcp_codec_same(packet+28,chaddr,6))
    return DHCP_CODEC_MISMATCH;
  if(packet[236]!=99u || packet[237]!=130u || packet[238]!=83u || packet[239]!=99u)
    return DHCP_CODEC_MALFORMED;
  dhcp_codec_copy(tmp.address,packet+16,4);
  while(pos<length) {
    unsigned tag=packet[pos++],n,bit=0;const uint8_t *value;
    if(tag==0u) continue;
    if(tag==255u) { ended=1;break; }
    if(pos==length) goto fail;
    n=packet[pos++];if(n>length-pos) goto fail;value=packet+pos;pos+=n;
    if(tag==52u) { rc=DHCP_CODEC_UNSUPPORTED;goto fail; }
    if(tag==53u) bit=1u; else if(tag==54u) bit=2u;
    else if(tag==1u) bit=4u; else if(tag==3u) bit=8u;
    else if(tag==6u) bit=16u; else if(tag==51u) bit=32u;
    else if(tag==58u) bit=64u; else if(tag==59u) bit=128u;
    if(bit) { if(seen&bit) goto fail;seen|=bit; }
    switch(tag) {
    case 53:
      if(n!=1u) goto fail;
      tmp.message_type=value[0];
      break;
    case 54:
      if(n!=4u || !dhcp_codec_ip_valid(value)) goto fail;
      dhcp_codec_copy(tmp.server,value,4);break;
    case 1:
      if(n!=4u || !dhcp_codec_mask_valid(value)) goto fail;
      dhcp_codec_copy(tmp.mask,value,4);tmp.present|=DHCP_CODEC_HAVE_MASK;break;
    case 3: case 6: {
      unsigned i;
      if(n==0u || (n%4u)!=0u) goto fail;
      for(i=0;i<n;i+=4u) if(!dhcp_codec_ip_valid(value+i)) goto fail;
      if(tag==3u) { dhcp_codec_copy(tmp.router,value,4);tmp.present|=DHCP_CODEC_HAVE_ROUTER; }
      else {
        dhcp_codec_copy(tmp.dns,value,4);tmp.present|=DHCP_CODEC_HAVE_DNS;
        /* ★ v71：第 2 条 DNS（option6 len=8）不再丢弃，供重试 failover 用 */
        if(n>=8u) { dhcp_codec_copy(tmp.dns2,value+4u,4u); tmp.dns2_present=1u; }
      }
      break;
    }
    case 51: case 58: case 59: {
      uint32_t seconds;if(n!=4u) goto fail;seconds=dhcp_codec_read32(value);
      if(!seconds) goto fail;
      if(tag==51u) { tmp.lease_seconds=seconds;tmp.present|=DHCP_CODEC_HAVE_LEASE; }
      else if(tag==58u) { tmp.renew_seconds=seconds;tmp.present|=DHCP_CODEC_HAVE_T1; }
      else { tmp.rebind_seconds=seconds;tmp.present|=DHCP_CODEC_HAVE_T2; }
      break;
    }
    default: break;
    }
  }
  if(!ended || (seen&3u)!=3u) goto fail;
  /* Only pad bytes may follow END; this prevents ambiguous trailing TLVs. */
  while(pos<length) if(packet[pos++]!=0u) goto fail;
  if(tmp.message_type!=DHCP_CODEC_OFFER && tmp.message_type!=DHCP_CODEC_ACK &&
     tmp.message_type!=DHCP_CODEC_NAK) { rc=DHCP_CODEC_MISMATCH;goto fail; }
  if(tmp.message_type==DHCP_CODEC_NAK) {
    uint8_t server_copy[4];dhcp_codec_copy(server_copy,tmp.server,4);
    dhcp_codec_clear(&tmp,sizeof(tmp));tmp.message_type=DHCP_CODEC_NAK;
    dhcp_codec_copy(tmp.server,server_copy,4);
  } else {
    if(!dhcp_codec_ip_valid(tmp.address)) goto fail;
    if((tmp.present&(DHCP_CODEC_HAVE_MASK|DHCP_CODEC_HAVE_ROUTER))==
       (DHCP_CODEC_HAVE_MASK|DHCP_CODEC_HAVE_ROUTER)) {
      unsigned i;for(i=0;i<4u;++i)
        if((tmp.address[i]&tmp.mask[i])!=(tmp.router[i]&tmp.mask[i])) goto fail;
    }
    if((tmp.present&(DHCP_CODEC_HAVE_LEASE|DHCP_CODEC_HAVE_T1))==
       (DHCP_CODEC_HAVE_LEASE|DHCP_CODEC_HAVE_T1) && tmp.renew_seconds>=tmp.lease_seconds) goto fail;
    if((tmp.present&(DHCP_CODEC_HAVE_LEASE|DHCP_CODEC_HAVE_T2))==
       (DHCP_CODEC_HAVE_LEASE|DHCP_CODEC_HAVE_T2) && tmp.rebind_seconds>=tmp.lease_seconds) goto fail;
    if((tmp.present&(DHCP_CODEC_HAVE_T1|DHCP_CODEC_HAVE_T2))==
       (DHCP_CODEC_HAVE_T1|DHCP_CODEC_HAVE_T2) && tmp.renew_seconds>=tmp.rebind_seconds) goto fail;
  }
  dhcp_codec_copy((uint8_t *)result,(const uint8_t *)&tmp,sizeof(tmp));return DHCP_CODEC_OK;
fail:
  return rc;
}
#endif
