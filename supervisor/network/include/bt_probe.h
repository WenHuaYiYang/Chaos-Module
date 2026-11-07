#ifndef SYSPATCH_BT_PROBE_H
#define SYSPATCH_BT_PROBE_H
#include <stdint.h>
void bt_probe_scan(void);
void bt_probe_action(uint32_t id);
void bt_probe_next(void);
void bt_probe_connect(void);
void bt_probe_disconnect(void);
void bt_probe_bnep(void);
void bt_probe_poll(void);
void bt_probe_text(char *out,uint32_t capacity);
void bt_probe_leave(void);
struct bt_probe_device {uint8_t address[6];int8_t rssi;char name[21];};
struct bt_probe_view {uint32_t phase,count,bnep,pair_confirm,passkey,scan_stage,error_stage;int32_t error;uint16_t cid,mtu;};
void bt_probe_view(struct bt_probe_view *out);
int bt_probe_device(uint32_t index,struct bt_probe_device *out);
void bt_probe_connect_device(uint32_t index);
int bt_probe_local_address(uint8_t address[6]);
struct bt_probe_diagnostics {
    uint32_t adapter_state,scan_stage,scan_events,scan_seen_start,command;
    int32_t ipc_result,service_result,sys_errno;
    uint32_t client_state;int32_t client_fd;
    uint32_t l2_event,l2_events;int32_t error;uint32_t error_stage;
};
void bt_probe_diagnostics(struct bt_probe_diagnostics *out);
#endif
