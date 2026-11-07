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

## 配对回调枚举与命令（2026-11-07 复盘 -611）

**根因**：`on_bond` 曾把"非 2 的值"一律判死，而 043 的 bond 状态是 **0/1/2 三态**：
固件里 bttool 的 bond 回调（0x0c6f281d，回调表槽 9）按 value 索引字符串表，
表在别名 VA `0x2CD48618` → 文件 0xC8618：

```
[0] BOND_NONE  [1] BONDING  [2] BONDED
```

所以新设备配对时栈先发 **BONDING(1)**，旧逻辑立刻 `-611`；已配对设备直接回
**BONDED(2)**，这就是"已配对能连、新设备必报 -611"的全部原因。

**修复后的语义**（bt_probe.c `on_bond`）：
- 2 → `connect_ready`，进入 L2CAP；
- 1 → 配对进行中，保持 PAIR、不重发 94、不判死；PAIR 超时放宽到 60s；
- 0 → 可能是重建配对前的旧密钥清理快照，节流重试（20 tick ≈ 5s），累计 8 次才 -611；
- 回调不再按 `transport==1` 过滤（q66 参考实现的假设，043 无设备证据），只按地址匹配。

**命令补充取证**（capstone 精确反汇编）：
- `0x0C3AFE64` = connect classic → `movs r2,#0x69`，payload 仅 6 字节地址（无 transport）；
- `0x0C3AFFA2` = create bond → `movs r2,#0x5e(94)`，payload 6 字节地址 + `payload[6]=transport`
  （1=BREDR，见 bttool 帮助 `create bond, params: <addr> <transport>(0:BLE, 1:BREDR)`）。

**地址序**：L2CAP connect 用扫描序在真机已实证可用，建绑定默认同序，连续失败 3 次后才试反转序。

## 配对卡死 / -16 复盘（2026-11-07 第二轮，真机截图驱动）

真机诊断事件序列（电脑配对）：

```
bond  BC:17:B8:BB:BD:8F  a=1 b=1   ← BONDING
pair  BC:17:B8:BB:BD:8F  a=0 b=0   ← 收到配对请求
bond  BC:17:B8:BB:BD:8F  a=1 b=0   ← 掉回 BOND_NONE
```

**两个独立故障**：

1. **卡在"正在配对"**：`bt_probe_poll` 的 PAIR 超时只写 `state.error=-614`，**不退出 phases**，
   于是 `network_service` 永远看到 `bt.phase!=0`。
2. **-16 (EBUSY)**：`network_service.c` 的 `error(-16,1/2)` 就是"上一条流程没结束"——
   电脑那次卡死的 PAIR 没清，再点手机必报 -16。

**IO capability 取证**：bttool 命令表项 `{help="params: <io capability> (0:displayonly, 1:yes&no,
2:keyboardonly, 3:no-in/no-out 4:keyboarddisplay)", name="iocap", handler=0x0C6F3971}`；
handler 调 `0x0C3AF700` → `movs r2,#0x29`，payload[0]=capability。

旧值 `1`(YesNo) 要求双方显示数字并确认，而 043 的 display 回调（槽 6 / 0x0C6F3B85）在实测中
**从未触发**，于是对端等不到确认 → 取消 → bond 回 0。改为 **3 (NoInputNoOutput / Just Works)**：
对端不需要我们显示或确认，收到 pair request 回 `0x61 accept=1` 即完成。

**命令再次核对**（capstone）：`0x0C3B0050` = pair request reply → cmd **0x61**，payload 6B 地址 +
`payload[6]`=accept；`0x0C3B008C` = confirmation → cmd **0x62**，payload 6B 地址 + `payload[6]`=transport
+ `payload[7]`=conform。

## 回调槽位与系统配对初始化（2026-11-07 第三轮，固件实证）

**槽位不再是推断**。固件里 `bt_adapter_register`（0x0C3AF3E8）有 10 个调用点，其中
btmain 的那张表在别名 VA `0x2CD4A744`，槽 5/6/9 直接解出日志串：

| 槽 | 函数 | 日志字符串 | 语义 |
|---|---|---|---|
| 5 | `0x0C709E91` | `gap_pair_request_callback` / `allow_link_connect_pair_request` | 配对请求，回 cmd **0x61**(accept) |
| 6 | `0x0C709C79` | `gap_pair_display_callback` / `Pair Display [%s][%s]` / `[SSP][NOTIFY][%lu]` | SSP 显示配对码 |
| 9 | `0x0C709B69` | bond 回调（0/1/2 三态） | 绑定状态 |

**系统配对初始化序列**（btmain `0x0C70B004`，此前我们只做了中间一条）：

```
cmd 0x27  u32 0x00280704      ; class of device
cmd 0x29  u8  3               ; IO capability = NoInputNoOutput (系统默认)
cmd 0x3d  {0,1}               ; 系统同值；缺失时配对会被对端直接取消
```

`bt_probe.c prepare()` 现在按同一序列下发一次（0x29 用 1=DisplayYesNo，让支持
Numeric Comparison 的对端能弹出配对码；请求仍自动 accept 兜底）。

对照命令号（capstone 逐个确认）：`0x25`=set scan mode{p0,p1}、`0x26`=get scan mode、
`0x27`=CoD、`0x29`=set io capability、`0x3d`={u8,u8}、`0x69`=connect classic(6B 地址)、
`0x5e(94)`=create bond(6B 地址+transport)、`0x61`=pair request reply(6B+accept)、
`0x62`=confirmation(6B+transport+conform)。

## 配对重试命令与诊断字段（2026-11-07 第四轮）

bttool 命令表项 `{help="cancel bond, params: <addr>", name="removebond", handler=0x0C6F2D31}`
→ SDK `0x0C3AFFDC` → **cmd 0x5F(95)**，payload = 6B 地址 + 1B transport。

用途：`on_bond` 收到 `value==0`（配对失败）时置 `bond_remove_pending`，下一拍先发
`0x5F` 删掉本机残留绑定再重配 —— 电脑/手机端保留旧配对记录时，`createbond(94)`
会被对方直接拒绝，现象就是 bond `1 → 0` 无限循环。

**配对确认双发**：`pair request` 到达时同时回
- `0x61 {addr, accept=1}`（pair request reply）
- `0x62 {addr, transport=1, conform=1}`（confirmation）

不同栈把 Numeric Comparison 的确认放在不同命令上，只回 0x61 时对端会一直等确认
直到取消。失败无害。

**诊断新增返回码**（`bt_probe_diagnostics` 124B）：`result_cod`(0x27)、`result_iocap`(0x29)、
`result_paircfg`(0x3d)、`result_connect`(0x69)、`result_bond`(94)、`result_confirm`(0x62)、
`result_remove`(0x5F)。>0 = 蓝牙服务拒绝，-618 = IPC 失败。

## 本地蓝牙名（2026-11-08）

bttool 命令表项 `{help="params: <local name>, example \"vela-bt\"", name="name", handler=0x0C6F3921}`
→ SDK `0x0C3AF5C8` → **cmd 0x22(34)**：payload = 名字字符串，固件内 `strncpy` 上限 `0x3F`(63)
并补 0（写 `msg+0x7F`），所以下发 64 字节（63 字符 + 终止符）即可。

`bt_probe.c prepare()` 的设备初始化块里下发 **"Chaos-Module"**（一次，`configured` 守卫），
返回码记入 `result_name`，诊断页“设备配置返回”行可见。

**补充（同日第二轮）**：`0x23` = get local name（返回 64 字节到 payload）。`bt_probe.c`
把名字设置从"一次性"改成**每次 `prepare()` 都重下发**并立刻用 `0x23` 回读：系统"设备名称"
被修改或重启会把 adapter 名顶回默认值（旧版本连手机显示 `Syspatch` 就是这个原因），
只在首次设置一次会被覆盖。回读值进诊断页"本地蓝牙名"行（附 `0x23` 返回码）。
