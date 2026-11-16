/*
 * TinyPAN Hardware Abstraction Layer
 * 
 * This header defines the interface between TinyPAN and the underlying
 * Bluetooth stack. To port TinyPAN to a new platform, implement all
 * functions declared in this file.
 */

#ifndef TINYPAN_HAL_H
#define TINYPAN_HAL_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ============================================================================
 * Constants
 * ============================================================================ */

/** Bluetooth device address length */
#define HAL_BD_ADDR_LEN     6

/** L2CAP PSM for BNEP */
#define HAL_BNEP_PSM        0x000F

/** Minimum L2CAP MTU for BNEP */
#define HAL_BNEP_MIN_MTU    1691

/* ============================================================================
 * Event Types
 * ============================================================================ */

/**
 * @brief L2CAP event types passed to the event callback
 */
typedef enum {
    HAL_L2CAP_EVENT_CONNECTED = 1,      /**< L2CAP channel opened successfully */
    HAL_L2CAP_EVENT_DISCONNECTED,       /**< L2CAP channel closed */
    HAL_L2CAP_EVENT_CONNECT_FAILED,     /**< L2CAP connection attempt failed */
    HAL_L2CAP_EVENT_CAN_SEND_NOW,       /**< Radio is ready to accept the next frame */
    HAL_L2CAP_EVENT_TX_COMPLETE         /**< Previous send_iovec call's data has been consumed by the radio.
                                             The transport layer uses this to release pbuf references.
                                             Must be fired once per successful send_iovec call.

                                             **STACK SAFETY WARNING:** To prevent deep stack recursion and
                                             overflow panics, the HAL MUST NOT fire this event immediately
                                             within the call stack of hal_bt_l2cap_send_iovec(). Instead,
                                             the HAL should set a flag and fire the event during the 
                                             subsequent hal_bt_poll() execution in the application task.

                                             On HALs that use DMA, fire from the TX-done ISR or bridge
                                             via a task-safe queue to the next poll cycle.
                                             NOTE: Do not fire for contiguous hal_bt_l2cap_send() calls. */
} hal_l2cap_event_t;

/* ============================================================================
 * Callback Types
 * ============================================================================ */

/**
 * @brief Callback for incoming L2CAP data
 * 
 * @param data      Pointer to received data
 * @param len       Length of received data
 * @param user_data User data pointer from registration
 */
typedef void (*hal_l2cap_recv_callback_t)(const uint8_t* data, uint16_t len, void* user_data);

/**
 * @brief Callback for L2CAP connection events
 * 
 * @param event     Event type
 * @param status    Status code (0 = success, non-zero = error)
 * @param user_data User data pointer from registration
 */
typedef void (*hal_l2cap_event_callback_t)(hal_l2cap_event_t event, int status, void* user_data);

/* ============================================================================
 * Bluetooth Functions
 * ============================================================================ */

/**
 * @brief Initialize the Bluetooth stack
 * 
 * Called once during tinypan_init(). Should initialize the underlying
 * Bluetooth stack and prepare it for L2CAP connections.
 * 
 * @return 0 on success, negative error code on failure
 */
int hal_bt_init(void);

/**
 * @brief Poll the Bluetooth stack for events
 * 
 * Called periodically during `tinypan_process()`. Should drain any internal
 * queues or handle background stack events to ensure they are processed
 * in the same thread context as TinyPAN.
 */
void hal_bt_poll(void);

/**
 * @brief De-initialize the Bluetooth stack
 * 
 * Called during tinypan_deinit(). Should clean up all Bluetooth resources.
 */
void hal_bt_deinit(void);

/**
 * @brief Connect to a remote device's L2CAP channel
 * 
 * Initiates an L2CAP connection to the specified device and PSM.
 * This is a non-blocking call. The result will be reported via the
 * event callback (HAL_L2CAP_EVENT_CONNECTED or HAL_L2CAP_EVENT_CONNECT_FAILED).
 * 
 * @param remote_addr   6-byte Bluetooth device address
 * @param psm           Protocol/Service Multiplexer (use HAL_BNEP_PSM for BNEP)
 * @param local_mtu     Maximum Transmission Unit to negotiate (minimum 1691 for BNEP)
 * @return 0 on success (connection initiated), negative error code on failure
 */
int hal_bt_l2cap_connect(const uint8_t remote_addr[HAL_BD_ADDR_LEN], uint16_t psm, uint16_t local_mtu);

/**
 * @brief Disconnect the current L2CAP channel
 * 
 * Closes the active L2CAP connection. The event callback will receive
 * HAL_L2CAP_EVENT_DISCONNECTED when complete.
 */
void hal_bt_l2cap_disconnect(void);

/**
 * @brief Send data over the L2CAP channel
 *
 * Sends a single contiguous buffer over the Bluetooth channel. Used primarily
 * for BNEP control packets and SLIP-mode data.
 *
 * This function relies exclusively on return values for backpressure (returns
 * 1 if busy). The HAL should NOT fire HAL_L2CAP_EVENT_TX_COMPLETE for this
 * function. Once it returns 0, the caller assumes the data has been copied.
 *
 * @param data         Pointer to the frame
 * @param len          Total length of the frame
 * @return 0 on success, negative error code on failure, 1 if radio is busy/congested
 */
int hal_bt_l2cap_send(const uint8_t* data, uint16_t len);

/**
 * @brief Scatter-gather I/O vector for zero-copy transmissions.
 */
typedef struct {
    const uint8_t* iov_base; /**< Pointer to data buffer */
    uint16_t iov_len;        /**< Length of data buffer */
} tinypan_iovec_t;

/**
 * @brief Send a scatter-gather array over the L2CAP channel
 * 
 * The preferred TX interface for BNEP mode. The iovec array describes
 * a logical frame split across non-contiguous memory regions (e.g., a
 * synthesized BNEP header in local storage and the original lwIP pbuf payload).
 *
 * **ASYNCHRONOUS CONTRACT:**
 * The HAL may copy the segments into an internal aligned buffer (bounce buffer) 
 * or map them into hardware DMA descriptors. The `iov` array and the buffers it 
 * points to MUST be treated as immutable and persistent by the HAL until
 * `HAL_L2CAP_EVENT_TX_COMPLETE` is fired. 
 *
 * TinyPAN guarantees that the `iov` descriptors passed here are stored in
 * static transport state (not the stack) for the duration of the transmission.
 *
 * **POINTER PERSISTENCE & FLUSHING:**
 * If the connection is lost or `tinypan_stop()` is called, TinyPAN will flush
 * its internal queues. However, TinyPAN will NOT free the pbuf for any
 * `in_flight` transmission. The HAL is guaranteed that the pointers in `iov`
 * will remain valid until the HAL fires `HAL_L2CAP_EVENT_TX_COMPLETE`.
 * The HAL must ensure it eventually fires this event for every successful
 * send call to prevent memory leaks in the stack.
 *
 * @param iov          Array of I/O vectors
 * @param iov_count    Number of vectors in the array
 * @return 0 on success, negative error code on failure, 1 if radio is busy/congested
 */
int hal_bt_l2cap_send_iovec(const tinypan_iovec_t* iov, uint16_t iov_count);

/**
 * @brief Check if the L2CAP channel is ready to send data
 * 
 * @return true if ready to send, false if busy
 */
bool hal_bt_l2cap_can_send(void);

/**
 * @brief Request a "can send now" event
 * 
 * If a send API returns 1 (Busy), the transport calls this function and waits
 * for the HAL_L2CAP_EVENT_CAN_SEND_NOW event before trying to send again.
 *
 * **CRITICAL WARNING:** To prevent 100% CPU deadlocks, the HAL MUST NOT
 * immediately fire the CAN_SEND_NOW event if the hardware is genuinely congested.
 * The HAL should only fire the event when a native hardware un-congestion interrupt
 * fires. If no native interrupt exists, the HAL must implement a polling delay 
 * (e.g., waiting 5-10ms) before firing the event to allow the radio time to drain.
 */
void hal_bt_l2cap_request_can_send_now(void);

/**
 * @brief Register callback for incoming L2CAP data
 * 
 * @param callback  Function to call when data is received
 * @param user_data User data pointer to pass to callback
 */
void hal_bt_l2cap_register_recv_callback(hal_l2cap_recv_callback_t callback, void* user_data);

/**
 * @brief Register callback for L2CAP events
 * 
 * @param callback  Function to call on events
 * @param user_data User data pointer to pass to callback
 */
void hal_bt_l2cap_register_event_callback(hal_l2cap_event_callback_t callback, void* user_data);

/* ============================================================================
 * System Functions
 * ============================================================================ */

/**
 * @brief Get the local Bluetooth device address
 * 
 * @param addr  Buffer to store the 6-byte address
 */
void hal_get_local_bd_addr(uint8_t addr[HAL_BD_ADDR_LEN]);

/**
 * @brief Get current time in milliseconds
 * 
 * Used for timeouts and timing. Must be monotonically increasing.
 * Wrap-around is acceptable (will be handled correctly).
 * 
 * @return Current time in milliseconds
 */
uint32_t hal_get_tick_ms(void);

/**
 * @brief Register a callback to explicitly wake the RTOS polling thread
 * 
 * @param callback  Function to call from RX/Event contexts to awake the thread
 * @param user_data User data pointer
 */
void hal_bt_set_wakeup_callback(void (*callback)(void*), void* user_data);

/**
 * @brief Query dynamic HAL timeout constraints (e.g. backoff timers)
 * @return Milliseconds until next mandatory poll, or 0xFFFFFFFF if none.
 */
uint32_t hal_bt_get_next_timeout_ms(void);

/**
 * @brief Get the negotiated Link MTU for chunking logic
 * @return The operational MTU limit in bytes.
 */
uint16_t hal_bt_l2cap_get_mtu(void);

/* ============================================================================
 * Thread Synchronization API
 * 
 * Required for RTOS environments (NO_SYS=0) where lwIP calls linkoutput()
 * from a different thread context than the main tinypan_process() loop.
 * ============================================================================ */

/** Opaque handle for a HAL-provided mutex */
typedef void* hal_mutex_t;

/**
 * @brief Create a new mutex
 * @return Handle to the mutex, or NULL on failure
 */
hal_mutex_t hal_mutex_create(void);

/**
 * @brief Acquire a mutex (blocking)
 * @param mutex Mutex handle
 */
void hal_mutex_lock(hal_mutex_t mutex);

/**
 * @brief Release a mutex
 * @param mutex Mutex handle
 */
void hal_mutex_unlock(hal_mutex_t mutex);

/**
 * @brief Destroy a mutex
 * @param mutex Mutex handle
 */
void hal_mutex_destroy(hal_mutex_t mutex);


#ifdef __cplusplus
}
#endif

#endif /* TINYPAN_HAL_H */
