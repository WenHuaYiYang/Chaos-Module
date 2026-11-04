#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "canopus_protocol.h"

int main(void)
{
    uint8_t buf[1024];
    uint8_t payload[] = { 2, 10, 'b','a','n','d','_','f','i','l','e','s', 1,2,3 };
    struct canopus_proto_request_v1 req;
    struct canopus_proto_response_v1 resp;
    uint32_t off = 0, opcode = 0;

    memset(&req, 0, sizeof(req));
    req.magic = CANOPUS_PROTO_MAGIC;
    req.struct_size = CANOPUS_PROTO_REQUEST_SIZE;
    req.abi_major = CANOPUS_ABI_MAJOR;
    req.command = CANOPUS_CMD_CONNECT_SEND;
    req.request_id = 7;
    req.payload_size = sizeof(payload);
    int n = canopus_transport_v2_encode_request(&req, payload, buf, sizeof(buf));
    assert(n > 0);
    memset(&req, 0, sizeof(req));
    assert(canopus_transport_v2_decode_request(buf, (uint32_t)n, &req, &off) == 0);
    assert(req.command == CANOPUS_CMD_CONNECT_SEND && req.request_id == 7);
    assert(req.payload_size == sizeof(payload));
    assert(memcmp(buf + off, payload, sizeof(payload)) == 0);

    canopus_proto_response_init(&resp, 7, CANOPUS_RESULT_COMPLETED, 1);
    n = canopus_transport_v2_encode_response(&resp, CANOPUS_CMD_CONNECT_SEND,
                                             payload, 1, buf, sizeof(buf));
    assert(n > 0);
    assert(canopus_transport_v2_decode_response(buf, (uint32_t)n, &resp, &opcode, &off) == 0);
    assert(opcode == CANOPUS_CMD_CONNECT_SEND && resp.request_id == 7);
    assert(resp.payload_size == 1 && buf[off] == 2);

    n = canopus_transport_v2_encode_event(CANOPUS_CMD_CONNECT_MESSAGE,
                                          payload, sizeof(payload), buf, sizeof(buf));
    assert(n > 0);
    assert(buf[0] == 0x32 && buf[1] == 0x43 && buf[2] == 0x50 && buf[3] == 0x43); /* 0x43504332 little-endian */
    printf("bridge protocol: OK\n");
    return 0;
}
