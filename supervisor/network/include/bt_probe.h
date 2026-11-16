#ifndef SYSPATCH_BT_PROBE_H
#define SYSPATCH_BT_PROBE_H
#include <stdint.h>
void bt_probe_scan(void);
void bt_probe_action(uint32_t id);
void bt_probe_next(void);
void bt_probe_connect(void);
int bt_probe_connect_address(const uint8_t address[6],const char *name);
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
    /* 配对取证: bond 回调枚举 0=BOND_NONE 1=BONDING 2=BONDED(固件字符串表实证)。 */
    uint32_t bond_attempts,bond_pending,bonding,bond_events,bond_value,bond_transport,bond_order,phase;
    uint32_t pair_reply_result;   /* cmd 0x61 的返回(非0=配对请求被拒/失败) */
    uint32_t device_configured;   /* CoD/io cap/0x3d 设备级配置已下发 */
    /* 关键命令的返回码(>0 = 蓝牙服务拒绝, -618 = IPC 失败), 排查用。 */
    int32_t result_cod,result_iocap,result_paircfg,result_connect,result_bond,result_confirm,result_remove,result_name,result_name_read;
    char local_name[32];          /* cmd 0x23 回读的 adapter 本地名 */
};
void bt_probe_diagnostics(struct bt_probe_diagnostics *out);
/* 最近 12 条蓝牙回调事件, 文本行; 返回写入字节数(不含终止符)。 */
uint32_t bt_probe_event_text(char *out,uint32_t capacity);
#endif
