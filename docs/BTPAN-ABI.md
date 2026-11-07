# Chaos BTPAN ABI / p67 3.101.043

目标固件：Xiaomi Band 10 Pro / p67 / 3.101.043。原始 AP SHA-256：
519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec，XIP 代码基址 0x0C0C0000。本文只记录该镜像的静态指令证据和主机模型测试，不把 Band 11/q66 地址或主机测试当作真机通过。

## 已核对地址

| 用途 | Thumb 入口或全局 | 证据 |
| --- | --- | --- |
| bt_socket_client_sendrecv | 0x0C3AC47D | adapter+0x30 连接，0x2C4 消息，非零写入结果走错误路径 |
| 注册回调 | 0x0C3AF3E9 | IPC 0x31，17 个 callback word |
| 取 adapter 状态 | 0x0C3AF4F5 | IPC 0x1B |
| discovery start | 0x0C3AF525 | IPC 0x1E，参数为 adapter、timeout |
| discovery stop | 0x0C3AF551 | IPC 0x1F |
| 取本机地址 | getter 0x0C3AF5A1；stock wrapper 0x0C701C71 | IPC 0x21，回复地址在 message+60 |
| adapter 全局 | 0x20126738 | 0x0C70B180 直接读取；0x2013DBE4 是另一个 miwear/cpevent 对象 |
| alloc/free | 0x0C840C55 / 0x0C840C61 | 043 Bluetooth allocator wrapper |
| pair request/display | 0x0C3B0051 / 0x0C3B008D | IPC 0x61 / 0x62 |
| ordinary CoC connect | 0x0C805B0D | 交给 0x0C7EB988 |
| ordinary CoC disconnect | 0x0C805BBD | 真正 disconnect wrapper；0x0C805B39 是 accept/reject handler |
| submit CID | 0x0C805BE9 | 将完整 32-bit CID 写入 buffer+4 |
| external queue | 0x0C7EB9CD | 五参数 owner、run、cancel、arg、flag |
| L2CAP owner | 0x2013BDBC | p67 exact owner root |

地址均为 Thumb callable 地址；反汇编偶地址减 1 后得到上表地址。生产头文件位于 supervisor/network/include/bt_addresses.h。

## Discovery start 返回 1 的含义

0x0C3AF524 先把 timeout 写入临时 0x2C4 消息的 message+60，调用 0x0C3AC47C(adapter,message,0x1E)，再将非零 sendrecv 返回值截断为低 8 位；只有 sendrecv 返回零时才读取 message+0x10 的服务状态。stock inquiry caller 0x0C6F3A00 只把 rc 等于 0 当成功，timeout 检查范围为 1 到 48 秒。

因此 UI 的 1/1 表示 state.error=1、error_stage=1，不是发现了一台设备。静态上有两种来源：

1. sendrecv 使用 adapter+0x30 的连接或 FD 写消息失败；043 的 sendrecv 对非 errno 4/11 的负写入结果清理后固定返回 1；
2. IPC 已发送，但服务在 message+0x10 返回状态 1。

仅凭 AP 静态代码不能进一步区分这两种情况。真机排查必须同时记录 adapter+0x30、sendrecv 原始返回、IPC reply+0x0c/+0x10，并确认 worker task 是否继承 btmain 所属任务组的 FD 表。不要把 1 映射成无设备。

## L2CAP connect、disconnect、queue

043 的 68-byte connect request 字段：PSM u16@+2（SDP=1，PAN=15）、callback u32@+12、地址 6@+16、MTU u16@+52（1691）、flag u16@+54。Assigned event 2 读取 CID u16@+0；Open event 3 读取 CID u16@+108 和远端 MTU u16@+72；Close event 6 读取 CID u16@+0、reason u16@+2、peer address +4；Data event 7 读取 total u16@+0、offset u16@+2、CID u16@+4，payload 在 +4+offset。

0x0C7EB9CD 的外部队列节点布局是 owner、arg、run、cancel、flag at +8、+12、+16、+20、+24。live owner 调 run，withdrawn owner 调 cancel；lock-null 路径同步调用 cancel(0,221,arg)。run/cancel 必须各自恰好释放一次 token。

0x0C805BE9 写完整 CID word，不是只写低 16 位。stock outgoing buffer 为 total u16@+0、payload offset u16@+2、metadata flag +8、payload +20。

## 主机证据

tools/probe_btpan_abi.py 在 Unicorn 上执行原始 043 指令并通过：adapter/IPC33、discovery/pair reply、callback registration、connect/disconnect/accept/submit wrappers、external queue enqueue/cancel/dispatcher、event 2/3/6 record producer。

tools/test_pan_network.py 当前 23 项全通过：真实 ARM lwIP/TinyPAN 虚拟以太网 DHCP/DNS/TCP、HTTP、入站 listener、坏 DNS/SYN 重传、拒绝/超时、24 次 DHCP 重连、6 个 TCP generation handle、SDP continuation/错误/取消/迟到 CID/所有权释放，以及 8 槽命令队列和最新 ON/OFF 目标保留。

## 真机边界

- 未在真实 3.101.043 手环上验证 adapter IPC FD 继承、扫描回调、配对、L2CAP CID、BNEP setup 和 DHCP；主机模型不能替代这些结果。
- bt_probe.c 和 pan_radio.c 的客户端上下文与 BNEP 生命周期由主线程维护；本文只给出 ABI 审查意见，不声称固件运行稳定。
- 若出现 1/1，优先抓 sendrecv/FD 原始值再决定是否修复任务组继承，禁止把该错误静默转换为成功扫描。
