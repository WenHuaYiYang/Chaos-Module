/*
 * TinyPAN BNEP Layer
 * 
 * Bluetooth Network Encapsulation Protocol: frame construction, 
 * control packet handling, and protocol state management.
 */

#include "tinypan_bnep.h"
#include "../include/tinypan_config.h"
#include "../include/tinypan_hal.h"
#include "../include/tinypan.h"
#if TINYPAN_ENABLE_LWIP
#include "lwip/pbuf.h"
#endif

#include <string.h>
/* Logging is disabled in the freestanding SysPatch port. */

/* ============================================================================
 * Static State
 * ============================================================================ */

/** Current BNEP state */
static bnep_state_t s_state = BNEP_STATE_CLOSED;

/** Local Ethernet/Bluetooth address */
static uint8_t s_local_addr[BNEP_ETHER_ADDR_LEN] = {0};

/** Remote Ethernet/Bluetooth address */
static uint8_t s_remote_addr[BNEP_ETHER_ADDR_LEN] = {0};

/** Cached compression flag to avoid redundant state checks in TX path */
static bool s_can_compress = false;

/** Frame receive callback */
static bnep_frame_recv_callback_t s_frame_callback = NULL;
static void* s_frame_callback_user_data = NULL;

/** State change callback */
static bnep_state_callback_t s_state_callback = NULL;
static void* s_state_callback_user_data = NULL;

/** Setup response callback */
static bnep_setup_response_callback_t s_setup_response_callback = NULL;
static void* s_setup_response_callback_user_data = NULL;

/** Filter response callback */
static bnep_filter_response_callback_t s_filter_response_callback = NULL;
static void* s_filter_response_callback_user_data = NULL;

/** Control packet queue sizing */
#define BNEP_CONTROL_QUEUE_LEN  4
#define BNEP_CONTROL_MAX_LEN    64

typedef struct {
    uint8_t buf[BNEP_CONTROL_MAX_LEN];
    uint8_t len;
} bnep_control_pkt_t;

static bnep_control_pkt_t s_control_queue[BNEP_CONTROL_QUEUE_LEN];
static uint8_t s_control_head = 0;
static uint8_t s_control_tail = 0;

/* ============================================================================
 * Helper Functions
 * ============================================================================ */

/**
 * @brief Write 16-bit value in big-endian (network byte order)
 */
static inline void write_be16(uint8_t* buffer, uint16_t value) {
    uint16_t be_val = TINYPAN_HTONS(value);
    memcpy(buffer, &be_val, 2);
}

/**
 * @brief Read 16-bit value safely from potentially unaligned buffer
 */
static inline uint16_t read_be16(const uint8_t* buffer) {
    uint16_t val;
    memcpy(&val, buffer, 2);
    return TINYPAN_NTOHS(val);
}

/**
 * @brief Set state and notify callback
 */
static void set_state(bnep_state_t new_state) {
    if (s_state != new_state) {
        TINYPAN_LOG_DEBUG("BNEP state: %d -> %d", s_state, new_state);
        s_state = new_state;
        if (s_state_callback) {
            s_state_callback(new_state, s_state_callback_user_data);
        }
    }
}

/* ============================================================================
 * Packet Building Functions
 * ============================================================================ */

int bnep_build_setup_request(uint8_t* buffer, uint16_t buffer_size,
                              uint16_t src_uuid, uint16_t dst_uuid) {
    /*
     * BNEP Setup Connection Request format:
     * 
     * Byte 0:      BNEP Type (0x01 = Control, no extension)
     * Byte 1:      Control Type (0x01 = Setup Connection Request)
     * Byte 2:      UUID Size (0x02 = 16-bit UUIDs)
     * Bytes 3-4:   Destination Service UUID (big-endian)
     * Bytes 5-6:   Source Service UUID (big-endian)
     * 
     * Total: 7 bytes
     */
    
    const uint16_t required_size = 7;
    
    if (buffer == NULL || buffer_size < required_size) {
        return -1;
    }
    
    buffer[0] = BNEP_PKT_TYPE_CONTROL;  /* No extension header */
    buffer[1] = BNEP_CTRL_SETUP_CONNECTION_REQUEST;
    buffer[2] = 0x02;  /* UUID size = 2 bytes (16-bit UUIDs) */
    write_be16(&buffer[3], dst_uuid);
    write_be16(&buffer[5], src_uuid);
    
    return (int)required_size;
}

int bnep_build_setup_response(uint8_t* buffer, uint16_t buffer_size,
                               uint16_t response_code) {
    /*
     * BNEP Setup Connection Response format:
     * 
     * Byte 0:      BNEP Type (0x01 = Control, no extension)
     * Byte 1:      Control Type (0x02 = Setup Connection Response)
     * Bytes 2-3:   Response Code (big-endian)
     * 
     * Total: 4 bytes
     */
    
    const uint16_t required_size = 4;
    
    if (buffer == NULL || buffer_size < required_size) {
        return -1;
    }
    
    buffer[0] = BNEP_PKT_TYPE_CONTROL;
    buffer[1] = BNEP_CTRL_SETUP_CONNECTION_RESPONSE;
    write_be16(&buffer[2], response_code);
    
    return (int)required_size;
}

int bnep_build_general_ethernet(uint8_t* buffer, uint16_t buffer_size,
                                 const uint8_t* dst_addr,
                                 const uint8_t* src_addr,
                                 uint16_t ethertype,
                                 const uint8_t* payload,
                                 uint16_t payload_len) {
    /*
     * BNEP General Ethernet format:
     * 
     * Byte 0:          BNEP Type (0x00 = General Ethernet)
     * Bytes 1-6:       Destination Address
     * Bytes 7-12:      Source Address
     * Bytes 13-14:     Networking Protocol Type (EtherType)
     * Bytes 15+:       Payload
     * 
     * Header size: 15 bytes
     */
    
    const uint16_t header_size = 15;
    const uint16_t required_size = header_size + payload_len;
    
    if (buffer == NULL || dst_addr == NULL || src_addr == NULL) {
        return -1;
    }
    
    if (buffer_size < required_size) {
        return -1;
    }
    
    buffer[0] = BNEP_PKT_TYPE_GENERAL_ETHERNET;  /* No extension */
    memcpy(&buffer[1], dst_addr, BNEP_ETHER_ADDR_LEN);
    memcpy(&buffer[7], src_addr, BNEP_ETHER_ADDR_LEN);
    write_be16(&buffer[13], ethertype);
    
    if (payload != NULL && payload_len > 0) {
        memcpy(&buffer[15], payload, payload_len);
    }
    
    return (int)required_size;
}

int bnep_build_compressed_ethernet(uint8_t* buffer, uint16_t buffer_size,
                                    uint16_t ethertype,
                                    const uint8_t* payload,
                                    uint16_t payload_len) {
    /*
     * BNEP Compressed Ethernet format (no addresses):
     * 
     * Byte 0:          BNEP Type (0x02 = Compressed Ethernet)
     * Bytes 1-2:       Networking Protocol Type (EtherType)
     * Bytes 3+:        Payload
     * 
     * Header size: 3 bytes
     */
    
    const uint16_t header_size = 3;
    const uint16_t required_size = header_size + payload_len;
    
    if (buffer == NULL || buffer_size < required_size) {
        return -1;
    }
    
    buffer[0] = BNEP_PKT_TYPE_COMPRESSED_ETHERNET;
    write_be16(&buffer[1], ethertype);
    
    if (payload != NULL && payload_len > 0) {
        memcpy(&buffer[3], payload, payload_len);
    }
    
    return (int)required_size;
}

int bnep_set_multicast_filters(const uint8_t (*filter_ranges)[12], uint16_t num_ranges) {
    if (s_state != BNEP_STATE_CONNECTED && s_state != BNEP_STATE_WAIT_FOR_CONNECTION_RESPONSE) {
        return -1;
    }

    if (num_ranges == 0 || filter_ranges == NULL) {
        return -1;
    }

    /* Calculate total payload size: 2 bytes (len) + (num_ranges * 12 bytes) */
    uint16_t payload_size = num_ranges * 12;
    
    if (payload_size + 4 > BNEP_CONTROL_MAX_LEN) {
        TINYPAN_LOG_ERROR("Multicast filter set too large: %u bytes", payload_size);
        return -1;
    }

    uint8_t pkt[BNEP_CONTROL_MAX_LEN];
    pkt[0] = BNEP_PKT_TYPE_CONTROL;
    pkt[1] = BNEP_CTRL_FILTER_MULTI_ADDR_SET;
    
    /* Write total length of filter list in bytes */
    write_be16(&pkt[2], payload_size);
    
    /* Copy filter ranges */
    memcpy(&pkt[4], filter_ranges, payload_size);

    /* Queue control packet */
    uint16_t total_len = 4 + payload_size;
    uint8_t next_tail = (s_control_tail + 1) % BNEP_CONTROL_QUEUE_LEN;
    if (next_tail == s_control_head) {
        return -1; /* Queue full */
    }

    bnep_control_pkt_t* q_pkt = &s_control_queue[s_control_tail];
    q_pkt->len = (uint8_t)total_len;
    memcpy(q_pkt->buf, pkt, total_len);
    s_control_tail = next_tail;

    hal_bt_l2cap_request_can_send_now();
    return 0;
}

/* ============================================================================
 * Packet Parsing Functions
 * ============================================================================ */

int bnep_parse_header(const uint8_t* data, uint16_t len,
                       uint8_t* pkt_type, bool* has_ext, uint16_t* header_len) {
    if (data == NULL || len < 1) {
        return -1;
    }
    
    uint8_t first_byte = data[0];
    *pkt_type = first_byte & BNEP_TYPE_MASK;
    *has_ext = (first_byte & BNEP_EXT_HEADER_FLAG) != 0;
    
    /* Calculate header length based on packet type */
    switch (*pkt_type) {
        case BNEP_PKT_TYPE_GENERAL_ETHERNET:
            /* Type(1) + DstAddr(6) + SrcAddr(6) + EtherType(2) = 15 */
            *header_len = 15;
            break;
            
        case BNEP_PKT_TYPE_CONTROL:
            /* Type(1). Extension headers might follow. */
            *header_len = 1;
            break;
            
        case BNEP_PKT_TYPE_COMPRESSED_ETHERNET:
            /* Type(1) + EtherType(2) = 3 */
            *header_len = 3;
            break;
            
        case BNEP_PKT_TYPE_COMPRESSED_SRC_ONLY:
            /* Type(1) + SrcAddr(6) + EtherType(2) = 9 */
            *header_len = 9;
            break;
            
        case BNEP_PKT_TYPE_COMPRESSED_DST_ONLY:
            /* Type(1) + DstAddr(6) + EtherType(2) = 9 */
            *header_len = 9;
            break;
            
        default:
            TINYPAN_LOG_WARN("Unknown BNEP packet type: 0x%02X", *pkt_type);
            return -1;
    }
    
    if (len < *header_len) {
        TINYPAN_LOG_WARN("BNEP packet too short: %u < %u", len, *header_len);
        return -1;
    }
    
    return 0;
}

int bnep_parse_ethernet_frame(const uint8_t* data, uint16_t len,
                               const uint8_t* local_addr,
                               const uint8_t* remote_addr,
                               bnep_ethernet_frame_t* frame) {
    if (data == NULL || frame == NULL || len < 3) {
        return -1;
    }
    
    uint8_t pkt_type;
    bool has_ext;
    uint16_t header_len;
    
    if (bnep_parse_header(data, len, &pkt_type, &has_ext, &header_len) < 0) {
        return -1;
    }
    
    uint32_t ext_offset = header_len;
    while (has_ext) {
        if (ext_offset + 2 > len) {
            TINYPAN_LOG_WARN("Packet too short for BNEP extension header");
            return -1;
        }
        uint8_t ext_type = data[ext_offset];
        uint8_t ext_len = data[ext_offset + 1];
        
        /* Strict Bounds Check: Ensure the extension length doesn't overflow the packet 
         * before we perform the addition, preventing potential OOB reads. */
        if (ext_offset + 2 + ext_len > len) {
            TINYPAN_LOG_WARN("BNEP extension header length exceeds packet size");
            return -1;
        }

        has_ext = (ext_type & BNEP_EXT_HEADER_FLAG) != 0;
        ext_offset += 2 + ext_len;
    }
    
    switch (pkt_type) {
        case BNEP_PKT_TYPE_GENERAL_ETHERNET:
            /* Full addresses in packet */
            memcpy(frame->dst_addr, &data[1], BNEP_ETHER_ADDR_LEN);
            memcpy(frame->src_addr, &data[7], BNEP_ETHER_ADDR_LEN);
            frame->ethertype = read_be16(&data[13]);
            break;
            
        case BNEP_PKT_TYPE_COMPRESSED_ETHERNET:
            /* No addresses - use local/remote */
            if (local_addr == NULL || remote_addr == NULL) {
                return -1;
            }
            /* In compressed mode: dst=local, src=remote (we're receiving) */
            memcpy(frame->dst_addr, local_addr, BNEP_ETHER_ADDR_LEN);
            memcpy(frame->src_addr, remote_addr, BNEP_ETHER_ADDR_LEN);
            frame->ethertype = read_be16(&data[1]);
            break;
            
        case BNEP_PKT_TYPE_COMPRESSED_SRC_ONLY:
            /* Only source address in packet, dst=local */
            if (local_addr == NULL) {
                return -1;
            }
            memcpy(frame->dst_addr, local_addr, BNEP_ETHER_ADDR_LEN);
            memcpy(frame->src_addr, &data[1], BNEP_ETHER_ADDR_LEN);
            frame->ethertype = read_be16(&data[7]);
            break;
            
        case BNEP_PKT_TYPE_COMPRESSED_DST_ONLY:
            /* Only destination address in packet, src=remote */
            if (remote_addr == NULL) {
                return -1;
            }
            memcpy(frame->dst_addr, &data[1], BNEP_ETHER_ADDR_LEN);
            memcpy(frame->src_addr, remote_addr, BNEP_ETHER_ADDR_LEN);
            frame->ethertype = read_be16(&data[7]);
            break;
            
        default:
            return -1;
    }
    
    /* Payload follows extension headers (if any) */
    frame->payload = &data[ext_offset];
    frame->payload_len = len - ext_offset;
    
    return 0;
}

int bnep_parse_setup_response(const uint8_t* data, uint16_t len,
                               bnep_setup_response_t* response) {
    /*
     * Setup Response format (after type byte):
     * Byte 0:      Control Type (0x02)
     * Bytes 1-2:   Response Code
     * 
     * We expect: Control Type at data[0], Response at data[1-2]
     */
    
    if (data == NULL || response == NULL || len < 3) {
        return -1;
    }
    
    if (data[0] != BNEP_CTRL_SETUP_CONNECTION_RESPONSE) {
        TINYPAN_LOG_WARN("Not a setup response: 0x%02X", data[0]);
        return -1;
    }
    
    response->response_code = read_be16(&data[1]);
    return 0;
}

/* ============================================================================
 * BNEP API Implementation
 * ============================================================================ */

void bnep_init(void) {
    s_state = BNEP_STATE_CLOSED;
    memset(s_local_addr, 0, sizeof(s_local_addr));
    memset(s_remote_addr, 0, sizeof(s_remote_addr));
    s_can_compress = false;
    s_frame_callback = NULL;
    s_state_callback = NULL;
    s_setup_response_callback = NULL;
    TINYPAN_LOG_INFO("BNEP initialized");
}

void bnep_reset(void) {
    set_state(BNEP_STATE_CLOSED);
    TINYPAN_LOG_DEBUG("BNEP reset");
}

void bnep_set_local_addr(const uint8_t addr[BNEP_ETHER_ADDR_LEN]) {
    if (addr != NULL) {
        memcpy(s_local_addr, addr, BNEP_ETHER_ADDR_LEN);
        /* Update cached compression flag */
        s_can_compress = (s_local_addr[0] != 0 || s_local_addr[5] != 0) && 
                         (s_remote_addr[0] != 0 || s_remote_addr[5] != 0);
        TINYPAN_LOG_DEBUG("BNEP local addr: %02X:%02X:%02X:%02X:%02X:%02X",
                          addr[0], addr[1], addr[2], addr[3], addr[4], addr[5]);
    }
}

void bnep_set_remote_addr(const uint8_t addr[BNEP_ETHER_ADDR_LEN]) {
    if (addr != NULL) {
        memcpy(s_remote_addr, addr, BNEP_ETHER_ADDR_LEN);
        /* Update cached compression flag */
        s_can_compress = (s_local_addr[0] != 0 || s_local_addr[5] != 0) && 
                         (s_remote_addr[0] != 0 || s_remote_addr[5] != 0);
        TINYPAN_LOG_DEBUG("BNEP remote addr: %02X:%02X:%02X:%02X:%02X:%02X",
                          addr[0], addr[1], addr[2], addr[3], addr[4], addr[5]);
    }
}

void bnep_register_frame_callback(bnep_frame_recv_callback_t callback, void* user_data) {
    s_frame_callback = callback;
    s_frame_callback_user_data = user_data;
}

void bnep_register_state_callback(bnep_state_callback_t callback, void* user_data) {
    s_state_callback = callback;
    s_state_callback_user_data = user_data;
}

void bnep_register_setup_response_callback(bnep_setup_response_callback_t callback, void* user_data) {
    s_setup_response_callback = callback;
    s_setup_response_callback_user_data = user_data;
}

void bnep_register_filter_response_callback(bnep_filter_response_callback_t callback, void* user_data) {
    s_filter_response_callback = callback;
    s_filter_response_callback_user_data = user_data;
}

bnep_state_t bnep_get_state(void) {
    return s_state;
}

bool bnep_is_connected(void) {
    return s_state == BNEP_STATE_CONNECTED;
}

int bnep_send_setup_request(void) {
    if (s_state != BNEP_STATE_WAIT_FOR_CONNECTION_RESPONSE && 
        s_state != BNEP_STATE_CLOSED) {
        TINYPAN_LOG_WARN("Cannot send setup request in state %d", s_state);
        /* Allow sending anyway for retries */
    }
    
    uint8_t tx_buffer[16];
    int pkt_len = bnep_build_setup_request(tx_buffer, sizeof(tx_buffer),
                                            BNEP_UUID_PANU, BNEP_UUID_NAP);
    if (pkt_len < 0) {
        TINYPAN_LOG_ERROR("Failed to build setup request");
        return -1;
    }
    
    TINYPAN_LOG_DEBUG("Sending BNEP setup request (PANU -> NAP)");
    
    int result = hal_bt_l2cap_send(tx_buffer, (uint16_t)pkt_len);
    if (result > 0) {
        TINYPAN_LOG_DEBUG("L2CAP busy, cannot send BNEP setup request");
        hal_bt_l2cap_request_can_send_now();
        return TINYPAN_ERR_BUSY;
    } else if (result < 0) {
        TINYPAN_LOG_ERROR("Failed to send setup request: %d", result);
        return result;
    }
    
    return 0;
}

int bnep_send_setup_response(uint16_t response_code) {
    uint8_t tx_buffer[8];
    int pkt_len = bnep_build_setup_response(tx_buffer, sizeof(tx_buffer),
                                             response_code);
    if (pkt_len < 0) {
        TINYPAN_LOG_ERROR("Failed to build setup response");
        return -1;
    }
    
    TINYPAN_LOG_DEBUG("Sending BNEP setup response: 0x%04X", response_code);
    
    int result = hal_bt_l2cap_send(tx_buffer, (uint16_t)pkt_len);
    if (result > 0) {
        TINYPAN_LOG_DEBUG("L2CAP busy, queuing BNEP setup response");
        if (pkt_len <= BNEP_CONTROL_MAX_LEN) {
            uint8_t next_tail = (s_control_tail + 1) % BNEP_CONTROL_QUEUE_LEN;
            if (next_tail != s_control_head) {
                memcpy(s_control_queue[s_control_tail].buf, tx_buffer, pkt_len);
                s_control_queue[s_control_tail].len = (uint8_t)pkt_len;
                s_control_tail = next_tail;
                hal_bt_l2cap_request_can_send_now();
                return 0; /* Successfully queued for later transmission */
            }
        }
        return -1; /* Queue full or packet too large */
    } else if (result < 0) {
        TINYPAN_LOG_ERROR("Failed to send setup response: %d", result);
        return result;
    }
    
    return 0;
}

bool bnep_drain_control_tx_queue(void) {
    while (s_control_head != s_control_tail) {
        bnep_control_pkt_t* pkt = &s_control_queue[s_control_head];
        int result = hal_bt_l2cap_send(pkt->buf, pkt->len);
        if (result == 0) {
            s_control_head = (s_control_head + 1) % BNEP_CONTROL_QUEUE_LEN;
        } else if (result > 0) {
            hal_bt_l2cap_request_can_send_now();
            return false; /* Still busy */
        } else {
            TINYPAN_LOG_ERROR("Failed to drain BNEP control packet: %d", result);
            s_control_head = (s_control_head + 1) % BNEP_CONTROL_QUEUE_LEN;
        }
    }
    return true; /* Queue empty or finished draining */
}

uint8_t bnep_get_ethernet_header_len(const uint8_t* dst_addr, const uint8_t* src_addr) {
    /* Use BNEP Compressed Ethernet (type 0x02, 3 bytes) when transmitting a frame
     * where both addresses match the established PANU<->NAP session addresses.
     * In this case both sides already know the addresses from the BD_ADDR exchange,
     * so including them again is redundant bandwidth.  Saves 12 bytes per packet
     * on a ~300-700 kbps Classic BT link, reducing both latency and radio-on time.
     *
     * Fall back to General Ethernet (type 0x00, 15 bytes) for any frame whose
     * addresses do not match — though in practice all outbound lwIP frames go to
     * the NAP's MAC address constructed at session setup. */
    /* Use cached s_can_compress flag and first-byte fast-path check
     * to avoid O(N) memcmp on every packet in the TX path. */
    if (s_can_compress && dst_addr && src_addr &&
        dst_addr[5] == s_remote_addr[5] && src_addr[5] == s_local_addr[5] &&
        memcmp(dst_addr, s_remote_addr, 6) == 0 &&
        memcmp(src_addr, s_local_addr, 6) == 0) {
        return 3; /* Compressed Ethernet: type(1) + EtherType(2) */
    }
    return 15; /* General Ethernet: type(1) + dst(6) + src(6) + EtherType(2) */
}

void bnep_write_ethernet_header(uint8_t* buffer, uint8_t header_len,
                                const uint8_t* dst_addr, const uint8_t* src_addr,
                                uint16_t ethertype) {
    if (header_len == 3) {
        bnep_build_compressed_ethernet(buffer, header_len, ethertype, NULL, 0);
    } else {
        bnep_build_general_ethernet(buffer, header_len, dst_addr, src_addr, ethertype, NULL, 0);
    }
}

/* ============================================================================
 * Incoming Packet Handling
 * ============================================================================ */

/**
 * @brief Handle incoming control packet
 */
static void handle_control_packet(const uint8_t* data, uint16_t len) {
    if (len < 1) {
        TINYPAN_LOG_WARN("Control packet too short");
        return;
    }
    
    /* In BNEP, the Control Type is at the start of the payload
     * (after the BNEP header and any extension headers). */
    uint8_t control_type = data[0];
    
    switch (control_type) {
        case BNEP_CTRL_SETUP_CONNECTION_REQUEST:
            TINYPAN_LOG_DEBUG("Received setup connection request");
            /* We are acting as PANU (client), not a server.
             * If we receive a setup request, reject it. */
            if (bnep_send_setup_response(BNEP_SETUP_RESPONSE_NOT_ALLOWED) != 0) {
                TINYPAN_LOG_WARN("Could not queue setup response (queue full); "
                                 "peer may time out and drop the link");
            }
            break;
            
        case BNEP_CTRL_SETUP_CONNECTION_RESPONSE:
            TINYPAN_LOG_DEBUG("Received setup connection response");
            if (s_state == BNEP_STATE_WAIT_FOR_CONNECTION_RESPONSE) {
                bnep_setup_response_t response;
                if (bnep_parse_setup_response(data, len, &response) == 0) {
                    TINYPAN_LOG_INFO("BNEP setup response: 0x%04X", response.response_code);
                    
                    if (response.response_code == BNEP_SETUP_RESPONSE_SUCCESS) {
                        set_state(BNEP_STATE_CONNECTED);
                    }
                    
                    if (s_setup_response_callback) {
                        s_setup_response_callback(response.response_code,
                                                   s_setup_response_callback_user_data);
                    } else {
                        TINYPAN_LOG_WARN("BNEP: Setup response callback is NULL!");
                    }
                } else {
                    TINYPAN_LOG_ERROR("Failed to parse setup response");
                }
            } else {
                TINYPAN_LOG_WARN("Unexpected setup response in state %d", s_state);
            }
            break;
            
        case BNEP_CTRL_FILTER_NET_TYPE_SET:
        case BNEP_CTRL_FILTER_MULTI_ADDR_SET:
            /* BNEP Protocol Compliance: Acknowledge filter requests with 'Success'
               to prevent session termination by host OS stacks (e.g. iOS)
               that require a valid response to keep the link active. */
            TINYPAN_LOG_DEBUG("Received filter set request (type 0x%02X), acknowledging Success", control_type);
            {
                uint8_t resp_type = (control_type == BNEP_CTRL_FILTER_NET_TYPE_SET) ?
                                    BNEP_CTRL_FILTER_NET_TYPE_RESPONSE :
                                    BNEP_CTRL_FILTER_MULTI_ADDR_RESPONSE;
                uint8_t resp[4] = {
                    BNEP_PKT_TYPE_CONTROL,
                    resp_type,
                    0x00, 0x00  /* Success */
                };
                /* SysPatch: data begins at Control Type, not Packet Type. */
                uint16_t bytes = len >= 3 ? ((uint16_t)data[1] << 8) | data[2] : 0;
                uint16_t stride = control_type == BNEP_CTRL_FILTER_NET_TYPE_SET ? 4 : 12;
                if (len < 3 || bytes > len - 3 || bytes % stride != 0)
                    resp[3] = 2; /* Invalid filter range, not success. */
                if (resp[3] == 0) {
                    for (uint16_t i = 0; i < bytes; i += stride)
                        if (memcmp(data + 3 + i, data + 3 + i + stride / 2, stride / 2) > 0)
                            resp[3] = 2;
                }
                int result = hal_bt_l2cap_send(resp, sizeof(resp));
                if (result > 0) {
                    TINYPAN_LOG_DEBUG("L2CAP busy, queuing filter response");
                    if (sizeof(resp) <= BNEP_CONTROL_MAX_LEN) {
                        uint8_t next_tail = (s_control_tail + 1) % BNEP_CONTROL_QUEUE_LEN;
                        if (next_tail != s_control_head) {
                            memcpy(s_control_queue[s_control_tail].buf, resp, sizeof(resp));
                            s_control_queue[s_control_tail].len = sizeof(resp);
                            s_control_tail = next_tail;
                        }
                    }
                    hal_bt_l2cap_request_can_send_now();
                } else if (result < 0) {
                    TINYPAN_LOG_ERROR("Failed to send filter response: %d", result);
                }
            }
            break;
            
        case BNEP_CTRL_FILTER_MULTI_ADDR_RESPONSE:
        case BNEP_CTRL_FILTER_NET_TYPE_RESPONSE:
            {
                /* Parse 2-byte response code from payload */
                uint16_t filter_resp = 0x0001; /* default: unsupported */
                if (len < 3) break;
                filter_resp = ((uint16_t)data[1] << 8) | data[2];
                TINYPAN_LOG_INFO("BNEP Filter response: 0x%04X", filter_resp);

                if (s_filter_response_callback) {
                    s_filter_response_callback(filter_resp, s_filter_response_callback_user_data);
                }
            }
            break;
            
        case BNEP_CTRL_COMMAND_NOT_UNDERSTOOD:
            TINYPAN_LOG_WARN("Remote didn't understand our command");
            break;
            
        default:
            TINYPAN_LOG_WARN("Unknown control type: 0x%02X", control_type);
            /* Send "not understood" response */
            {
                uint8_t resp[3] = {
                    BNEP_PKT_TYPE_CONTROL,
                    BNEP_CTRL_COMMAND_NOT_UNDERSTOOD,
                    control_type
                };
                hal_bt_l2cap_send(resp, sizeof(resp));
            }
            break;
    }
}

/**
 * @brief Handle incoming Ethernet frame
 */
static void handle_ethernet_frame(const uint8_t* data, uint16_t len) {
    if (s_state != BNEP_STATE_CONNECTED) {
        TINYPAN_LOG_WARN("Received frame but not connected");
        return;
    }
    
    bnep_ethernet_frame_t frame;
    
    if (bnep_parse_ethernet_frame(data, len, s_local_addr, s_remote_addr, &frame) < 0) {
        TINYPAN_LOG_WARN("Failed to parse Ethernet frame");
        return;
    }
    
    TINYPAN_LOG_DEBUG("Received frame: ethertype=0x%04X len=%u", 
                       frame.ethertype, frame.payload_len);
    
    if (s_frame_callback) {
        s_frame_callback(&frame, s_frame_callback_user_data);
    }
}

void bnep_handle_incoming(const uint8_t* data, uint16_t len) {
    if (data == NULL || len == 0) {
        return;
    }
    
    uint8_t pkt_type;
    bool has_ext;
    uint16_t header_len;
    
    if (bnep_parse_header(data, len, &pkt_type, &has_ext, &header_len) < 0) {
        TINYPAN_LOG_WARN("Failed to parse BNEP header");
        return;
    }
    
    /* SysPatch: a control message precedes extension headers. Validate its
     * complete variable length before locating extensions or dispatching it. */
    uint16_t control_len = 0;
    if (pkt_type == BNEP_PKT_TYPE_CONTROL) {
        if (len < 2) return;
        switch (data[1]) {
            case BNEP_CTRL_COMMAND_NOT_UNDERSTOOD: control_len = 2; break;
            case BNEP_CTRL_SETUP_CONNECTION_REQUEST:
                if (len < 3 || (data[2] != 2 && data[2] != 4 && data[2] != 16)) return;
                control_len = 2 + 2 * data[2]; break;
            case BNEP_CTRL_SETUP_CONNECTION_RESPONSE:
            case BNEP_CTRL_FILTER_NET_TYPE_RESPONSE:
            case BNEP_CTRL_FILTER_MULTI_ADDR_RESPONSE: control_len = 3; break;
            case BNEP_CTRL_FILTER_NET_TYPE_SET:
            case BNEP_CTRL_FILTER_MULTI_ADDR_SET:
                if (len < 4) return;
                { uint32_t size = 3u + ((uint16_t)data[2] << 8) + data[3];
                  if (size > (uint32_t)len - 1) return; control_len = (uint16_t)size; }
                break;
            default: if (has_ext) return; control_len = len - 1; break;
        }
        if ((uint32_t)control_len + 1 > len) return;
    }
    uint32_t ext_offset = pkt_type == BNEP_PKT_TYPE_CONTROL ? 1u + control_len : header_len;
    uint32_t ext_start = ext_offset;
    bool extensions = has_ext;
    while (has_ext) {
        if (ext_offset + 2 > len) {
            TINYPAN_LOG_WARN("Packet too short for BNEP extensions");
            return;
        }
        uint8_t ext_type = data[ext_offset];
        uint8_t ext_len = data[ext_offset + 1];
        
        if (ext_offset + 2 + ext_len > len) {
            TINYPAN_LOG_WARN("BNEP extension length exceeds packet size");
            return;
        }

        has_ext = (ext_type & BNEP_EXT_HEADER_FLAG) != 0;
        ext_offset += 2 + ext_len;
    }

    if (pkt_type == BNEP_PKT_TYPE_CONTROL) handle_control_packet(data + 1, control_len);
    /* Process known control extensions only after the whole chain passed
     * bounds validation. Unknown extension types are skipped as specified. */
    while (extensions) {
        uint8_t type = data[ext_start], size = data[ext_start + 1];
        if ((type & BNEP_TYPE_MASK) == 0) handle_control_packet(data + ext_start + 2, size);
        extensions = (type & BNEP_EXT_HEADER_FLAG) != 0;
        ext_start += 2 + size;
    }

    switch (pkt_type) {
        case BNEP_PKT_TYPE_CONTROL:
            break;
            
        case BNEP_PKT_TYPE_GENERAL_ETHERNET:
        case BNEP_PKT_TYPE_COMPRESSED_ETHERNET:
        case BNEP_PKT_TYPE_COMPRESSED_SRC_ONLY:
        case BNEP_PKT_TYPE_COMPRESSED_DST_ONLY:
            handle_ethernet_frame(data, len);
            break;
            
        default:
            TINYPAN_LOG_WARN("Unknown BNEP packet type: 0x%02X", pkt_type);
            break;
    }
}

void bnep_on_l2cap_connected(void) {
    TINYPAN_LOG_INFO("L2CAP connected, sending BNEP setup request");
    set_state(BNEP_STATE_WAIT_FOR_CONNECTION_RESPONSE);
    bnep_send_setup_request();
}

void bnep_on_l2cap_disconnected(void) {
    TINYPAN_LOG_INFO("L2CAP disconnected");
    set_state(BNEP_STATE_CLOSED);
}
