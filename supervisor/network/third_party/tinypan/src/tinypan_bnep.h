/*
 * TinyPAN BNEP Layer - Internal Header
 * 
 * Bluetooth Network Encapsulation Protocol implementation.
 */

#ifndef TINYPAN_BNEP_H
#define TINYPAN_BNEP_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ============================================================================
 * BNEP Constants
 * ============================================================================ */

/** Ethernet address length */
#define BNEP_ETHER_ADDR_LEN         6

/** Standard Ethernet MTU */
#define BNEP_ETHERNET_MTU           1500

/** Maximum BNEP header size (type + dst + src + ethertype) */
#define BNEP_MAX_HEADER_SIZE        15

/* ----------------------------------------------------------------------------
 * BNEP Packet Types (lower 7 bits of first byte)
 * ---------------------------------------------------------------------------- */
#define BNEP_PKT_TYPE_GENERAL_ETHERNET          0x00
#define BNEP_PKT_TYPE_CONTROL                   0x01
#define BNEP_PKT_TYPE_COMPRESSED_ETHERNET       0x02
#define BNEP_PKT_TYPE_COMPRESSED_SRC_ONLY       0x03
#define BNEP_PKT_TYPE_COMPRESSED_DST_ONLY       0x04

/** Mask for packet type (bits 0-6) */
#define BNEP_TYPE_MASK                          0x7F

/** Extension header flag (bit 7) */
#define BNEP_EXT_HEADER_FLAG                    0x80

/* ----------------------------------------------------------------------------
 * BNEP Control Message Types
 * ---------------------------------------------------------------------------- */
#define BNEP_CTRL_COMMAND_NOT_UNDERSTOOD        0x00
#define BNEP_CTRL_SETUP_CONNECTION_REQUEST      0x01
#define BNEP_CTRL_SETUP_CONNECTION_RESPONSE     0x02
#define BNEP_CTRL_FILTER_NET_TYPE_SET           0x03
#define BNEP_CTRL_FILTER_NET_TYPE_RESPONSE      0x04
#define BNEP_CTRL_FILTER_MULTI_ADDR_SET         0x05
#define BNEP_CTRL_FILTER_MULTI_ADDR_RESPONSE    0x06

/* ----------------------------------------------------------------------------
 * BNEP Setup Connection Response Codes
 * ---------------------------------------------------------------------------- */
#define BNEP_SETUP_RESPONSE_SUCCESS             0x0000
#define BNEP_SETUP_RESPONSE_INVALID_DST         0x0001
#define BNEP_SETUP_RESPONSE_INVALID_SRC         0x0002
#define BNEP_SETUP_RESPONSE_INVALID_SVC         0x0003
#define BNEP_SETUP_RESPONSE_NOT_ALLOWED         0x0004

/* ----------------------------------------------------------------------------
 * BNEP Filter Response Codes
 * ---------------------------------------------------------------------------- */
#define BNEP_FILTER_RESPONSE_SUCCESS            0x0000
#define BNEP_FILTER_RESPONSE_UNSUPPORTED        0x0001
#define BNEP_FILTER_RESPONSE_INVALID_RANGE      0x0002
#define BNEP_FILTER_RESPONSE_LIMIT_REACHED      0x0003
#define BNEP_FILTER_RESPONSE_SECURITY_BLOCK     0x0004

/* ----------------------------------------------------------------------------
 * PAN Service UUIDs (16-bit short form)
 * ---------------------------------------------------------------------------- */
#define BNEP_UUID_PANU                          0x1115
#define BNEP_UUID_NAP                           0x1116
#define BNEP_UUID_GN                            0x1117

/* ----------------------------------------------------------------------------
 * EtherTypes (Network Protocol Types)
 * ---------------------------------------------------------------------------- */
#define BNEP_ETHERTYPE_IPV4                     0x0800
#define BNEP_ETHERTYPE_ARP                      0x0806
#define BNEP_ETHERTYPE_IPV6                     0x86DD

/* ============================================================================
 * BNEP State Machine
 * ============================================================================ */

/**
 * @brief BNEP channel states
 */
typedef enum {
    BNEP_STATE_CLOSED = 0,              /**< Channel closed */
    BNEP_STATE_WAIT_FOR_CONNECTION_REQUEST,  /**< Server waiting for request */
    BNEP_STATE_WAIT_FOR_CONNECTION_RESPONSE, /**< Client waiting for response */
    BNEP_STATE_CONNECTED                /**< Channel open and ready */
} bnep_state_t;

/* ============================================================================
 * BNEP Packet Structures
 * ============================================================================ */

/**
 * @brief Parsed BNEP Ethernet frame
 */
typedef struct {
    uint8_t  dst_addr[BNEP_ETHER_ADDR_LEN]; /**< Destination MAC address */
    uint8_t  src_addr[BNEP_ETHER_ADDR_LEN]; /**< Source MAC address */
    uint16_t ethertype;                      /**< EtherType (protocol) */
    const uint8_t* payload;                  /**< Pointer to payload data */
    uint16_t payload_len;                    /**< Payload length */
} bnep_ethernet_frame_t;

/**
 * @brief BNEP setup connection response
 */
typedef struct {
    uint16_t response_code;             /**< Response code */
} bnep_setup_response_t;

/* ============================================================================
 * Callback Types
 * ============================================================================ */

/**
 * @brief Callback for received Ethernet frames
 */
typedef void (*bnep_frame_recv_callback_t)(const bnep_ethernet_frame_t* frame, void* user_data);

/**
 * @brief Callback for BNEP state changes
 */
typedef void (*bnep_state_callback_t)(bnep_state_t new_state, void* user_data);

/**
 * @brief Callback for BNEP setup response
 */
typedef void (*bnep_setup_response_callback_t)(uint16_t response_code, void* user_data);

/**
 * @brief Callback for BNEP filter response
 */
typedef void (*bnep_filter_response_callback_t)(uint16_t response_code, void* user_data);

/* ============================================================================
 * BNEP API Functions
 * ============================================================================ */

/**
 * @brief Initialize BNEP layer
 */
void bnep_init(void);

/**
 * @brief Reset BNEP layer to closed state
 */
void bnep_reset(void);

/**
 * @brief Set local Ethernet address (used for BNEP)
 * 
 * @param addr 6-byte Ethernet address
 */
void bnep_set_local_addr(const uint8_t addr[BNEP_ETHER_ADDR_LEN]);

/**
 * @brief Set remote Ethernet address (used for BNEP)
 * 
 * @param addr 6-byte Ethernet address
 */
void bnep_set_remote_addr(const uint8_t addr[BNEP_ETHER_ADDR_LEN]);

/**
 * @brief Register callback for received Ethernet frames
 */
void bnep_register_frame_callback(bnep_frame_recv_callback_t callback, void* user_data);

/**
 * @brief Register callback for state changes
 */
void bnep_register_state_callback(bnep_state_callback_t callback, void* user_data);

/**
 * @brief Register callback for setup response
 */
void bnep_register_setup_response_callback(bnep_setup_response_callback_t callback, void* user_data);

/**
 * @brief Register callback for filter response
 */
void bnep_register_filter_response_callback(bnep_filter_response_callback_t callback, void* user_data);

/**
 * @brief Get current BNEP state
 */
bnep_state_t bnep_get_state(void);

/**
 * @brief Check if BNEP channel is connected
 */
bool bnep_is_connected(void);

/* ----------------------------------------------------------------------------
 * BNEP Packet Sending
 * ---------------------------------------------------------------------------- */

/**
 * @brief Send BNEP setup connection request
 * 
 * Sends: PANU (src) -> NAP (dst)
 * 
 * @return 0 on success, TINYPAN_ERR_BUSY if the radio is temporarily full
 *         (caller should retry on HAL_L2CAP_EVENT_CAN_SEND_NOW), negative on error
 */
int bnep_send_setup_request(void);

/**
 * @brief Send BNEP setup connection response
 * 
 * @param response_code Response code to send
 * @return 0 on success, TINYPAN_ERR_BUSY if the radio is temporarily full, negative on error
 */
int bnep_send_setup_response(uint16_t response_code);

/**
 * @brief Send a BNEP multicast address filter set request
 * 
 * @param filter_ranges  Pointer to an array of MAC address ranges (start, end)
 * @param num_ranges     Number of ranges in the array
 * @return 0 on success, negative error code on failure
 */
int bnep_set_multicast_filters(const uint8_t (*filter_ranges)[12], uint16_t num_ranges);

/**
 * @brief Drain the pending control packet queue
 * 
 * Called by tinypan_netif_drain_tx_queue.
 * 
 * @return true if queue is empty or successfully drained, false if still busy.
 */
bool bnep_drain_control_tx_queue(void);

/**
 * @brief Get the BNEP header length for an Ethernet frame
 * 
 * Determines whether compression can be applied based on the addresses.
 * 
 * @param dst_addr      Destination MAC address (6 bytes)
 * @param src_addr      Source MAC address (6 bytes)
 * @return Length of the BNEP header (15 or 3 bytes)
 */
uint8_t bnep_get_ethernet_header_len(const uint8_t* dst_addr, const uint8_t* src_addr);

/**
 * @brief Write the BNEP header into an existing buffer
 * 
 * @param buffer        Pointer to write the header
 * @param header_len    Header length returned by bnep_get_ethernet_header_len
 * @param dst_addr      Destination MAC address
 * @param src_addr      Source MAC address
 * @param ethertype     EtherType (e.g., 0x0800 for IPv4)
 */
void bnep_write_ethernet_header(uint8_t* buffer, uint8_t header_len,
                                const uint8_t* dst_addr, const uint8_t* src_addr,
                                uint16_t ethertype);

/* ----------------------------------------------------------------------------
 * BNEP Packet Receiving (called by HAL)
 * ---------------------------------------------------------------------------- */

/**
 * @brief Handle incoming L2CAP data
 * 
 * Called by HAL receive callback. Parses BNEP packets and dispatches
 * to appropriate handlers.
 * 
 * @param data  Pointer to received data
 * @param len   Length of received data
 */
void bnep_handle_incoming(const uint8_t* data, uint16_t len);

/**
 * @brief Called when L2CAP channel is opened
 * 
 * Transitions state machine and initiates setup handshake.
 */
void bnep_on_l2cap_connected(void);

/**
 * @brief Called when L2CAP channel is closed
 */
void bnep_on_l2cap_disconnected(void);

/* ----------------------------------------------------------------------------
 * BNEP Packet Building (Low-level - for unit testing)
 * ---------------------------------------------------------------------------- */

/**
 * @brief Build a BNEP setup connection request packet
 * 
 * @param buffer        Buffer to write packet to
 * @param buffer_size   Size of buffer
 * @param src_uuid      Source UUID (PANU = 0x1115)
 * @param dst_uuid      Destination UUID (NAP = 0x1116)
 * @return Length of packet, or negative on error
 */
int bnep_build_setup_request(uint8_t* buffer, uint16_t buffer_size,
                              uint16_t src_uuid, uint16_t dst_uuid);

/**
 * @brief Build a BNEP setup connection response packet
 * 
 * @param buffer        Buffer to write packet to
 * @param buffer_size   Size of buffer
 * @param response_code Response code
 * @return Length of packet, or negative on error
 */
int bnep_build_setup_response(uint8_t* buffer, uint16_t buffer_size,
                               uint16_t response_code);

/**
 * @brief Build a BNEP general Ethernet packet
 * 
 * @param buffer        Buffer to write packet to
 * @param buffer_size   Size of buffer
 * @param dst_addr      Destination MAC address
 * @param src_addr      Source MAC address
 * @param ethertype     EtherType
 * @param payload       Payload data
 * @param payload_len   Payload length
 * @return Length of packet, or negative on error
 */
int bnep_build_general_ethernet(uint8_t* buffer, uint16_t buffer_size,
                                 const uint8_t* dst_addr,
                                 const uint8_t* src_addr,
                                 uint16_t ethertype,
                                 const uint8_t* payload,
                                 uint16_t payload_len);

/**
 * @brief Build a BNEP compressed Ethernet packet (no addresses)
 * 
 * @param buffer        Buffer to write packet to
 * @param buffer_size   Size of buffer
 * @param ethertype     EtherType
 * @param payload       Payload data
 * @param payload_len   Payload length
 * @return Length of packet, or negative on error
 */
int bnep_build_compressed_ethernet(uint8_t* buffer, uint16_t buffer_size,
                                    uint16_t ethertype,
                                    const uint8_t* payload,
                                    uint16_t payload_len);

/* ----------------------------------------------------------------------------
 * BNEP Packet Parsing (Low-level - for unit testing)
 * ---------------------------------------------------------------------------- */

/**
 * @brief Parse a BNEP packet header
 * 
 * @param data          Pointer to BNEP packet
 * @param len           Length of packet
 * @param pkt_type      [out] Packet type
 * @param has_ext       [out] Whether extension headers are present
 * @param header_len    [out] Length of BNEP header
 * @return 0 on success, negative on error
 */
int bnep_parse_header(const uint8_t* data, uint16_t len,
                       uint8_t* pkt_type, bool* has_ext, uint16_t* header_len);

/**
 * @brief Parse a BNEP Ethernet packet into a frame structure
 * 
 * @param data          Pointer to BNEP packet
 * @param len           Length of packet
 * @param local_addr    Local Ethernet address (for compressed packets)
 * @param remote_addr   Remote Ethernet address (for compressed packets)
 * @param frame         [out] Parsed frame structure
 * @return 0 on success, negative on error
 */
int bnep_parse_ethernet_frame(const uint8_t* data, uint16_t len,
                               const uint8_t* local_addr,
                               const uint8_t* remote_addr,
                               bnep_ethernet_frame_t* frame);

/**
 * @brief Parse a BNEP setup response
 * 
 * @param data          Pointer to BNEP control packet (after type byte)
 * @param len           Length of data
 * @param response      [out] Parsed response structure
 * @return 0 on success, negative on error
 */
int bnep_parse_setup_response(const uint8_t* data, uint16_t len,
                               bnep_setup_response_t* response);

#ifdef __cplusplus
}
#endif

#endif /* TINYPAN_BNEP_H */
