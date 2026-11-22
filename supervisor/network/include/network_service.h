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
/* 记住的对端直连(不走 inquiry); 成功入队返回 0, 忙/未启用返回 -16。 */
int network_connect_address(const uint8_t address[6],const char *name);
void network_disconnect(void);
void network_pair_confirm(void);
void network_enable(int enabled);
/* 设置本地蓝牙名(手机蓝牙列表里看到的名字)。UI 线程只入队, 真正的 BT IPC
 * 由 PAN worker 执行 —— adapter 只在 worker 里创建, 跨线程直调会返回 -601。
 * 0 = 已入队; -22 名字非法/为空; 队列满时静默丢弃(调用方按拍重试)。 */
int network_set_bt_name(const char *name);
/* 最近一次设置结果: -999 = 尚无结果, 0 = 成功, 其它为 bt_probe 返回码。 */
int network_bt_name_result(void);
/* 队列里还有待办命令(worker 据此临时加快 BT 拍)。 */
int network_pending_commands(void);
void network_service_poll(void);
void network_service_view(struct network_view *);
/* One independent PSM1 channel. Poll/start/result only on PAN worker. */
int network_sdp_start(const uint8_t address[6]);
void network_sdp_poll(void);
int network_sdp_result(void); /* -11 pending, 0 absent, 1 NAP, other negative error */
void network_sdp_cancel(void);
#endif
