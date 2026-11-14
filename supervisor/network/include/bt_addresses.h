#ifndef BT_ADDRESSES_H
#define BT_ADDRESSES_H
/* p67tc 3.101.043 BT IPC 客户端 ABI —— 全部地址经固件取证，禁止填猜值。
 * 取证记录：docs/BTPAN-ABI.md / tools/probe_btpan_abi.py。
 * IPC 命令字与 q66 4.100.188 完全一致：33=取本机地址 37=设模式 38=取模式
 * 92=取 bond 态 94=建 bond（bt_probe.c request() 原样使用，不需要独立地址）。 */
#define BT_SOCKET_CLIENT_SENDRECV 0x0c3ac47du
/* Explicit per-task-group client constructor. It owns its socket and native
 * receive/event-loop task; do not borrow btmain's adapter slot. */
#define BT_CLIENT_CREATE           0x0c3af1a9u
#define BT_ERRNO_LOCATION          0x0c1e45bdu
#define BT_ADAPTER_REGISTER       0x0c3af3e9u
#define BT_ADAPTER_GET_STATE      0x0c3af4f5u
#define BT_DISCOVERY_START        0x0c3af525u
#define BT_DISCOVERY_STOP         0x0c3af551u
#define BT_ALLOC                  0x0c840c55u
#define BT_FREE                   0x0c840c61u
#define BT_PAIR_REQUEST_REPLY     0x0c3b0051u
#define BT_PAIR_DISPLAY_REPLY     0x0c3b008du
#define BT_ADAPTER_SLOT           0x20126738u
/* 注意: bt_adapter_set_scan_mode(0x0c3b56a9) 曾在 v65559 被直接调用(3,1),
 * 真机当场黑屏 —— 那是 PENDING/not_probed 的地址 + 未验证签名, 已撤除。
 * 要再试必须先在设备上做单点取证(最小模块 + 只调这一条), 不许夹在配对流程里。 */

/* 10 Pro / p67 3.101.043 classic PAN ABI. These are the entries recovered
 * from the exact AP call sites; they are not Band 11/q66 addresses. The
 * L2CAP owner is the classic L2CAP FSM root, not the adapter cpevent node. */
#define BT_L2CAP_CONNECT          0x0c805b0du
#define BT_L2CAP_DISCONNECT       0x0c805bbdu
#define BT_L2CAP_SUBMIT_CID       0x0c805be9u
#define BT_QUEUE_EXTERNAL         0x0c7eb9cdu
#define BT_QUEUE_FREE             0x0c840c61u
#define BT_OWNER_SLOT             0x2013bdbcu
#endif

