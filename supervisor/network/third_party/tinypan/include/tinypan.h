/*
 * TinyPAN - Bluetooth PAN Client Library
 * 
 * A portable PAN (Personal Area Network) client library for embedded systems.
 * Supports Bluetooth Classic (BNEP) and BLE (SLIP) operating modes.
 *
 * Copyright (c) 2026
 * Licensed under MIT License
 */

#ifndef TINYPAN_H
#define TINYPAN_H

#include <stdint.h>
#include <stdbool.h>

#include "tinypan_config.h"

#ifdef __cplusplus
extern "C" {
#endif

/* ============================================================================
 * Version Information
 * ============================================================================ */

#define TINYPAN_VERSION_MAJOR   0
#define TINYPAN_VERSION_MINOR   1
#define TINYPAN_VERSION_PATCH   0

/* ============================================================================
 * Type Definitions
 * ============================================================================ */

/**
 * @brief Bluetooth device address (6 bytes)
 */
typedef uint8_t tinypan_bd_addr_t[6];

/**
 * @brief Connection state enumeration
 */
typedef enum {
    TINYPAN_STATE_IDLE = 0,         /**< Not started, waiting */
    TINYPAN_STATE_SCANNING,         /**< Reserved, not implemented */
    TINYPAN_STATE_CONNECTING,       /**< L2CAP connection in progress */
    TINYPAN_STATE_BNEP_SETUP,       /**< BNEP setup handshake in progress */
    TINYPAN_STATE_BNEP_FILTER_WAIT, /**< Waiting for BNEP multicast filter response */
    TINYPAN_STATE_DHCP,             /**< BNEP negotiated, DHCP in progress (BNEP mode only) */
    TINYPAN_STATE_ONLINE,           /**< Transport ready for data transfer */
    TINYPAN_STATE_STALLED,          /**< Reserved, not implemented */
    TINYPAN_STATE_RECONNECTING,     /**< Connection lost, attempting reconnect */
    TINYPAN_STATE_ERROR             /**< Max reconnect attempts exhausted */
} tinypan_state_t;

/**
 * @brief Event types for callbacks
 */
typedef enum {
    TINYPAN_EVENT_STATE_CHANGED,    /**< State has changed */
    TINYPAN_EVENT_CONNECTED,        /**< Transport connection established (BNEP or SLIP) */
    TINYPAN_EVENT_DISCONNECTED,     /**< Connection lost */
    TINYPAN_EVENT_IP_ACQUIRED,      /**< IP address obtained via DHCP */
    TINYPAN_EVENT_IP_LOST,          /**< IP address lost */
    TINYPAN_EVENT_ERROR             /**< An error occurred */
} tinypan_event_t;

/**
 * @brief Error codes
 */
typedef enum {
    TINYPAN_OK = 0,                 /**< Success */
    TINYPAN_ERR_INVALID_PARAM = -1, /**< Invalid parameter */
    TINYPAN_ERR_NOT_INITIALIZED = -2, /**< Library not initialized */
    TINYPAN_ERR_ALREADY_STARTED = -3, /**< Already started */
    TINYPAN_ERR_NOT_STARTED = -4,   /**< Not started */
    TINYPAN_ERR_HAL_FAILED = -5,    /**< HAL function failed */
    TINYPAN_ERR_BNEP_FAILED = -6,   /**< BNEP error */
    TINYPAN_ERR_TIMEOUT = -7,       /**< Operation timed out */
    TINYPAN_ERR_NO_MEMORY = -8,     /**< Out of memory */
    TINYPAN_ERR_BUSY = -9           /**< Resource busy. Note: HAL layer returns positive 1 for busy. */
} tinypan_error_t;

/**
 * @brief Configuration structure
 */
typedef struct {
    tinypan_bd_addr_t remote_addr;  /**< Bluetooth address of NAP (phone) */
    uint16_t reconnect_interval_ms; /**< Initial reconnection delay (default: 1000) */
    uint16_t reconnect_max_ms;      /**< Maximum reconnection delay (default: 30000) */
    uint16_t heartbeat_interval_ms; /**< Link monitoring interval (default: 15000). Not implemented. */
    uint8_t  heartbeat_retries;     /**< Retries before declaring link dead (default: 3). Not implemented. */
    uint8_t  max_reconnect_attempts;/**< Maximum reconnect attempts, 0 = infinite (default: 0) */
    bool     auto_init_lwip;        /**< If true, TinyPAN calls lwip_init(). Set false if host OS manages lwIP. (default: true) */
} tinypan_config_t;

/**
 * @brief IP address information
 */
typedef struct {
    uint32_t ip_addr;               /**< IP address (network byte order) */
    uint32_t netmask;               /**< Network mask (network byte order) */
    uint32_t gateway;               /**< Gateway address (network byte order) */
    uint32_t dns_server;            /**< DNS server (network byte order) */
} tinypan_ip_info_t;

/**
 * @brief Event callback function type
 * 
 * @param event     The event that occurred
 * @param user_data User data pointer passed to tinypan_set_event_callback
 */
typedef void (*tinypan_event_callback_t)(tinypan_event_t event, void* user_data);

/* ============================================================================
 * Public API Functions
 * ============================================================================ */

/**
 * @brief Get default configuration values
 * 
 * @param config Pointer to configuration structure to fill with defaults
 */
void tinypan_config_init(tinypan_config_t* config);

/**
 * @brief Initialize TinyPAN library
 * 
 * Must be called once at startup before any other TinyPAN functions.
 * 
 * @param config Pointer to configuration structure
 * @return TINYPAN_OK on success, error code otherwise
 */
tinypan_error_t tinypan_init(const tinypan_config_t* config);

/**
 * @brief Register event callback
 * 
 * @param callback  Callback function to be called on events
 * @param user_data User data pointer to pass to callback
 */
void tinypan_set_event_callback(tinypan_event_callback_t callback, void* user_data);

/**
 * @brief Register a generic callback to wake the application's processing thread.
 * 
 * Invoked by the HAL from ISRs or event hooks to immediately abort sleep,
 * reducing network latency.
 */
void tinypan_set_wakeup_callback(void (*callback)(void*), void* user_data);

/**
 * @brief Start connection process
 * 
 * Begins the connection sequence to the configured NAP device.
 * 
 * @return TINYPAN_OK on success, error code otherwise
 */
tinypan_error_t tinypan_start(void);

/**
 * @brief Stop and disconnect
 * 
 * Disconnects any active connection and stops the library.
 */
void tinypan_stop(void);

/**
 * @brief Process TinyPAN events
 * 
 * Must be called periodically from main loop or a timer.
 * Handles state machine transitions and timeouts.
 */
void tinypan_process(void);

/**
 * @brief Get milliseconds until next timer event
 * 
 * Used by bare-metal (NO_SYS) or RTOS implementations to know exactly
 * how long the CPU can sleep (WFI/WFE) before `tinypan_process()` needs
 * to be called again. Prevents 100% active-CPU polling.
 * 
 * @return Milliseconds to sleep, or 0xFFFFFFFF for infinite
 */
uint32_t tinypan_get_next_timeout_ms(void);

/**
 * @brief Get current connection state
 * 
 * @return Current state
 */
tinypan_state_t tinypan_get_state(void);

/**
 * @brief Get state name as string (for debugging)
 * 
 * @param state State to convert
 * @return String representation of state
 */
const char* tinypan_state_to_string(tinypan_state_t state);

/**
 * @brief Check if online (IP acquired and link healthy)
 * 
 * @return true if online and ready for data transfer
 */
bool tinypan_is_online(void);

/**
 * @brief Get IP address information
 * 
 * @param info Pointer to structure to fill with IP info
 * @return TINYPAN_OK if IP is available, error otherwise
 */
tinypan_error_t tinypan_get_ip_info(tinypan_ip_info_t* info);

/**
 * @brief De-initialize TinyPAN library
 * 
 * Frees all resources and resets state.
 */
void tinypan_deinit(void);

#ifdef __cplusplus
}
#endif

#endif /* TINYPAN_H */
