#ifndef SYSPATCH_PAN_ETHERNET_H
#define SYSPATCH_PAN_ETHERNET_H
#include <stdint.h>
#include "../third_party/tinypan/src/tinypan_bnep.h"
#define PAN_ETH_MTU 1500u
#define PAN_ETH_FRAME_MAX 1514u
#define PAN_ETH_QUEUE_SIZE 8u
/* This is OUR adapter contract, NOT the unrecovered q66 net_driver_s ABI.
 * An adapter creates a real protocol-stack interface and returns its handle.
 * r15 uses our module's lwIP netif, not a native q66 system netdev.
 * All callbacks except tx are serialized on the PAN network worker. input
 * borrows the frame until return: 0 consumes it, -11 requests retry, other
 * negatives drop it. No callback takes ownership of module storage. */
struct pan_eth_backend {
    int (*create)(const char *name,const uint8_t *mac,unsigned mtu,
                  int (*tx)(const uint8_t *,unsigned),void **handle);
    void *(*find)(const char *name);
    int (*link)(void *handle,int up);
    int (*input)(void *handle,const uint8_t *frame,unsigned length);
    void (*destroy)(void *handle);
};
struct pan_eth_stats {
    uint32_t active,rx_pending,tx_pending,rx_packets,tx_packets;
    uint32_t rx_drops,tx_retries;
    int32_t last_error;
    char name[16];
};
/* start/stop/rx_poll must run on the SAME network worker, not BT callbacks.
 * bt_poll must run on the existing Bluetooth owner, never a UI timer.
 * No backend is installed automatically; NULL does not create a fake pan0. */
int pan_eth_start(const struct pan_eth_backend *,const uint8_t local_mac[6],const uint8_t remote_mac[6]);
void pan_eth_stop(void);
int pan_eth_tx(const uint8_t *frame,unsigned length);
void pan_eth_receive(const bnep_ethernet_frame_t *frame,void *unused);
void pan_eth_rx_poll(void);
void pan_eth_bt_poll(void);
void pan_eth_snapshot(struct pan_eth_stats *out);
#endif
