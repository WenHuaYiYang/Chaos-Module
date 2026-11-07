#ifndef SYSPATCH_NETWORK_SERVICE_H
#define SYSPATCH_NETWORK_SERVICE_H
#include "bt_probe.h"
enum {NETWORK_IDLE,NETWORK_SCANNING,NETWORK_DISCOVERING,NETWORK_CONNECTING,NETWORK_PAIRING,NETWORK_NEGOTIATING,NETWORK_DHCP,NETWORK_READY};
struct network_peer {struct bt_probe_device device;unsigned probe_index;};
struct network_view {
    unsigned phase,count,checked,candidates,selected,passkey,error_serial;
    int error;unsigned error_stage;uint16_t cid,mtu;
    char connected_name[32];uint8_t address[6];
    struct network_peer peers[16];
    unsigned enabled;
};
/* UI sends commands; only PAN worker executes Bluetooth operations. */
void network_scan(void);
void network_connect(unsigned index);
void network_disconnect(void);
void network_pair_confirm(void);
void network_enable(int enabled);
void network_service_poll(void);
void network_service_view(struct network_view *);
/* One independent PSM1 channel. Poll/start/result only on PAN worker. */
int network_sdp_start(const uint8_t address[6]);
void network_sdp_poll(void);
int network_sdp_result(void); /* -11 pending, 0 absent, 1 NAP, other negative error */
void network_sdp_cancel(void);
#endif
