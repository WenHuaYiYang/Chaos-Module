/* ChaosConnect v1. Private file protocol; independent of CPC2/CanopusConnect. */
#ifndef CHAOS_CONNECT_H
#define CHAOS_CONNECT_H
#include <stdint.h>
#define CC_VERSION 1u
#define CC_HEADER 16u
#define CC_CHUNK 2048u
#define CC_MAX_PAYLOAD (CC_CHUNK + 256u)
#define CC_MAX_FRAME (CC_HEADER + CC_MAX_PAYLOAD)
#define CC_MAX_WIRE (CC_MAX_FRAME + 96u)
#define CC_WINDOW 8u
#define CC_PB_TYPE 0u /* WearPacket.Type.OTHER; private identity is inside CCN1. */
#define CC_RESPONSE 1u
#define CC_EVENT 2u
struct cc_frame { uint8_t flags,op,status; uint16_t req,len; const uint8_t *payload; };
uint32_t cc_crc32(uint32_t crc,const void *data,uint32_t len);
int cc_encode(uint8_t *out,uint32_t cap,uint8_t flags,uint8_t op,uint8_t status,uint16_t req,const void *data,uint32_t len);
int cc_decode(const uint8_t *data,uint32_t len,struct cc_frame *out);
/* Documented WearPacket/ThirdpartyApp MessageContent envelope, not a host extension.
 * TYPE must be a known enum: host pbjson logging rejects unknown enum values.
 * Existing host prost decode/re-encode retains the known MessageContent bytes field.
 * Returns 1 only for this namespace, 0 for unrelated traffic, -1 for malformed data. */
int cc_pb_unwrap(const uint8_t *data,uint32_t len,const uint8_t **frame,uint32_t *size,uint32_t *id);
int cc_pb_wrap(uint8_t *out,uint32_t cap,const uint8_t *frame,uint32_t len,uint32_t id);
int cc_reply(uint8_t flags,uint8_t op,uint8_t status,uint16_t req,const void *data,uint32_t len);
void cc_files_dispatch(const struct cc_frame *frame);
void cc_files_reset(void);
void cc_files_set_chunk(uint16_t chunk);
#endif
