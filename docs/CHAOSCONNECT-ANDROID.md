# ChaosConnect Android：连接确认、权限与普通蓝牙（p67 / 3.101.043）

2026-11-15。目标 AP SHA-256：
`519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec`。
原 AstroBox 插件载波保留；Android 新增标准经典蓝牙 SPP/RFCOMM 载波，
不依赖小米账号认证、AstroBox 私有会话、BTPAN 或网页服务器。
本轮为手机后续功能提供连接、权限状态与受控文件 API；设备控制位预留，尚无控制指令。

## 使用

1. 手环：主页 → 信息 → ChaosConnect → **发起蓝牙配对**，自动开启 ChaosConnect。
2. 手机打开系统蓝牙设置保持可见。可从手机搜索手环配对；也可在手环点“扫描可配对手机”，
   点选设备，由手环发送普通 BR/EDR connect/create-bond 请求。无匹配手机时不猜地址。
3. Android ChaosConnect → 添加设备 → 搜索手环，授予“附近设备”权限。
   点选手环，使用系统配对界面完成配对，再建立专用服务连接。
4. 手环默认弹出 **是否允许 Android ChaosConnect 连接**；唤醒屏幕、震动一次。
   拒绝或 60 秒未确认均不能访问文件。蓝牙配对本身不是 CCN1 授权。
5. 权限：ChaosConnect → 权限设置 → Android端权限。
   “允许Android连接”有开启/关闭/询问，默认询问；五项权限如下。

| 位 | 权限 | 默认 | 当前执行点 |
| --- | --- | --- | --- |
| 1 | 读取文件 | 开 | LIST/STAT/OPEN_READ/READ；COPY 同时要求读取、写入 |
| 2 | 写入文件 | 关 | MKDIR/OPEN_WRITE/WRITE/COMMIT/COPY |
| 4 | 删除和重命名 | 关 | REMOVE/RMDIR/RENAME；可独立于写入权限开启 |
| 8 | 开发者模式 | 关 | SET_DEV(1)，仍须通过原开发者策略的允许/询问 |
| 16 | 设备控制 | 关 | 预留；没有实现或开放任何控制 opcode |

手机不能设置这些权限。权限改变后每个请求重新检查，owner 维护取消对应读取/未提交写入；
复制循环和提交替换原文件前再次检查。已传到对端的数据不能收回，已完成的原子操作不回滚。
文件引擎仍拒绝 `/dev`、`/proc`、`/sys`，普通模式仍只写 `/data`。
授权配置及其临时文件、祖先路径始终禁止远程改写/移动。

Android 配置独立于插件：`/data/chaos/connect-android.cfg`，7 字节：
`CCA1`、connection policy u8、mask u8、`policy XOR mask XOR 0xa5`。
policy=0允许/1拒绝/2询问；临时文件为同目录 `.tmp`，完整写入/关闭后 rename；
错误/缺失回到询问、只读，保存失败保留原策略。插件原 `CCP1` 配置不迁移。

## 协议扩展（CCN1 版本仍为 1）

Android HELLO payload 固定 `CCA1`，HELLO 应答仍是原 11 字节，新增能力位 `0x08`。
Android 必须检查能力位，旧模块缺少此位时拒绝连接。
原空 HELLO 插件应答布局不变，可忽略新能力位继续使用。

原标准 PB namespace、CCN1 header、CRC、req/prefix、2048B chunk 和 8 块窗口不变。
SPP 接收端把**物理载波来源**传给文件引擎；即使原始蓝牙客户端谎发空 HELLO，
也会被强制标记为 Android 并执行权限，不能借旧插件握手绕过确认。
已经认证的小米载波仍受其自身的认证边界保护；`CCA1` 是协议声明，不是应用身份证明。
SPP UUID 也不是口令，授权以手环的本次会话决定为准，不做永久设备白名单。

新增请求 `GET_ACCESS=0x42`，空 payload；应答 16 字节：

| 偏移 | 字段 |
| --- | --- |
| 0 | access schema = 1 |
| 1 | Android client = 0/1 |
| 2 | connection policy = 0/1/2 |
| 3 | connection authorization = 0无/1等待/2允许/3拒绝/4超时 |
| 4 | permission mask u32LE |
| 8 | 本次连接确认 token u32LE |
| 12 | 本次连接已授权 u8 |
| 13..15 | 保留零 |

`DISCONNECT=0x43` 撤销会话、清理文件。客户端通常关闭 socket 即可。
确认前只接受 HELLO/PING/GET_ACCESS/DISCONNECT，其他操作返回 ACCESS=3。
原 PING 的 8 字节布局及开发者模式授权保持兼容；连接 token 与开发者 token 共用递增
30-bit 发行序列，但由独立状态记录，旧 token、断连、重开均不能授权新会话。
GET_ACCESS 成功与 PING 成功不写逐次 INFO，避免轮询淹没日志。

Rust 原子状态附加 bit5=Android，bit6..8=连接授权状态；提示邮箱优先连接 token，
完成后再服务开发者授权。弹窗在 UI timer 上创建；文件操作在 file owner；配对/服务管理在独立 `chaos.connect` 任务，UV数据流在 BT loop。
Android 显示 AWAITING_APPROVAL，确认后才提供 `ChaosFiles` 并保存真实设备到 Room。
心跳通常3秒、等待确认时1秒，设备15秒无合法请求清理，连接/开发者确认最多60秒。
Android 查询并展示手环权限；本地检查只是交互辅助，最终门禁始终在手环。

## 标准蓝牙载波与固件证据

Android 使用公开 `BluetoothDevice.createRfcommSocketToServiceRecord(UUID)`，由系统完成
普通蓝牙配对和安全连接；不反射隐藏的 classic L2CAP API。RFCOMM 底层使用经典蓝牙 L2CAP。
专用 UUID：`43434e31-0001-4a10-8000-4348414f5301`，服务通道19、最多1连接；
官方 miwear 的 SPP 通道5保持不变。通道占用/注册失败明确报错，不覆盖别的服务。

SPP 是流，每条记录为 `u32LE WearPacket长度 + 完整WearPacket`，最大 `CC_MAX_WIRE`。
接收支持分片/粘包、超限拒绝、有界队列；队列满关闭本次载波，不静默丢请求。
发送一次仅允许一笔 native 异步 write，完成回调前保持缓冲，失败不覆盖在途数据。
借用读缓冲在返回 native 回调前复制，因为 native 随后释放它。
Android socket 连接/发送取消时 close 以中断阻塞调用，不在 UI 线程读写 socket。

| ABI（Thumb） | 原固件证据 |
| --- | --- |
| `0x0c3af1a9` | 独立分配0xd4 client、IPC接收与当前任务组注册；不借系统 adapter 的 SPP app |
| `0x0c3b14d1` | SPP app 注册，原表2槽连接状态、3槽proxy；原表 `0x2cd4b9c0` |
| `0x0c3b152d` | server start，IPC0xf4；20B UUID@payload+4，SCN@+24，max_connection@+26 |
| `0x0c3b1589` / `0x0c3b1625` | server stop IPC0xf5 / 按地址与port断开 IPC0xf7 |
| `0x0c3af64d` | scan mode IPC37，两个字节 `{mode,bondable}`；IPC38读旧值 |
| IPC0x69 / 94 | BR/EDR connect地址6B / create-bond地址6B+transport=1 |
| IPC0x61 / 0x62 | 配对请求与SSP确认；仅用户开启的窗口内响应本次peer |
| `0x0c3a9cad` | 连接SPP proxy提供的命名pipe；native callback `(pipe,status,cookie)` |
| `0x0c3a9a75` | read start；native借用回调 `(pipe,buffer,nread)` |
| `0x0c3a9bfd` | 异步write；完成回调 `(pipe,original_buffer,status)` |
| `0x0c3aa051` | 关闭native pipe、异步释放；发送缓冲另等write完成 |

配对窗口60秒后恢复之前的 scan mode；恢复失败关闭窗口接受门禁并保留重试，
不因restore失败无限开放本模块的配对请求。关闭模块时撤销服务、关闭专用pipe和profile连接。
服务随原启用开关启动，重启后已配对手机能重连；不自动进入可发现状态。
BT/owner空闲时降频，发送队列出现新应答再加速。蓝牙回调不调用LVGL或文件引擎。
只有owner对配对/载波状态变化记RAM日志。

分析产物在固件目录 `Ghidra/chaos-connect-*.txt`，AP映射基址是 `0x0c0c0000`；
constructor还按 `local` 名和当前任务组注册：不得在系统btmain组重复创建，故使用独立任务，避免原SPP app槽冲突。
SDK类型接口另核对 openvela 的 `framework/include/bt_spp.h`，实际结构/地址以本固件为准。

参考：[Android标准蓝牙连接](https://developer.android.com/develop/connectivity/bluetooth/connect-bluetooth-devices)，
[openvela SPP接口](https://github.com/open-vela/frameworks_bluetooth/blob/dev/framework/include/bt_spp.h)。

## 验证与边界

```sh
python3 tools/test_chaos_connect.py
/tmp/chaos-palm-verify/bin/python tools/probe_connect_radio_abi.py
RUSTUP_TOOLCHAIN=nightly /tmp/chaos-palm-verify/bin/python tools/probe_chaos_connect.py research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin
python3 tools/build.py --firmware 10p-3.101.043
# Android项目：
CHAOS_MODULE_ROOT=/path/to/Chaos-Module ./gradlew testDebugUnitTest assembleDebug lintDebug
```

主机25项：生产C文件引擎/真实临时VFS、原插件回归、Android等待/允许/拒绝/超时、
来源不可降级、独立权限、撤权清理、策略保护、Rust持久化/保存失败/提示邮箱/唤醒震动、
标准蓝牙状态机和流分片/粘包/背压/关闭。固件探针执行真实SPP注册/服务/断开/配对/UV包装指令；
IPC服务、heap、libuv提交、无线是明确替身。旧ARM载波探针仍覆盖原认证通道及关闭/重开。
Android14项JVM测试（其中4项生产C互通）覆盖会话、数据传输和权限撤销。

尚未在真实3.101.043手环和实体Android手机配对，不能确认无线可靠性、功耗或200kb/s速度。
前端文件管理/远程操控页面尚未增加；现有文件API可供后续页面使用，控制权限位暂不执行动作。
