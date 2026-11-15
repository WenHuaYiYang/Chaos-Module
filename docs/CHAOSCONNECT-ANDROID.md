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
接收端把**物理载波来源**传给文件引擎：SPP为Android，原认证小米载波为插件；即使原始蓝牙客户端谎发空 HELLO，
也会被强制标记为 Android 并执行权限，不能借旧插件握手绕过确认。
已经认证的小米载波仍受其自身的认证边界保护；`CCA1` 是协议声明，不是应用身份证明。
插件发送 `CCA1` 也仍按插件权限执行，不能借此切换到Android权限。
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

## 进入配对页误报“蓝牙错误1”的修复

用户实机反馈：点“发起蓝牙配对”立即报1。核对 `0x0c3af680` 原指令与反编译，
IPC38把当前 scan mode 放在 reply+12，函数直接返回这个枚举；不是通用的状态码。
0=不可连接，1=可连接，2=可连接且可发现。旧通用IPC封装把1/2按失败处理，
且错误地从payload读取两字节旧值。修正为读取reply+12并接受0/1/2，独立保留IPC失败，
恢复可见性使用保存的mode；getter不提供bondable值，setter按原固件调用惯例使用true。

增加生产C状态机的三种mode回归，mock现在按真实ABI把mode写reply+12并污染旧payload，
防止测试继续掩盖错误。原指令探针执行getter并验证0/1/2逐值返回。
其他命令的非零status仍为错误；页面现在显示注册服务、查询可见性、开启配对窗口、
扫描或数据流等具体失败阶段。此项修复仍需实机重新开启配对窗口确认。

## 扫到手机后配对失败、手机点选后设备消失的修正

进一步核对原固件：`0x0c709e90` 的系统pair-request回调会读取
`0x201264a4`（名单门禁）和 `0x201264a6`（原允许地址），名单开启且地址不匹配时
立即发送0x61拒绝，独立client的异步accept无法撤销这次拒绝。
模块仅在用户开启的60秒配对窗口暂时释放该RAM门禁；关闭/超时/真正BR绑定成功后恢复。
不改原允许地址、不删除已有绑定，ChaosConnect会话仍须手环确认，文件权限不受影响。
原指令探针执行系统callback，验证名单拒绝→窗口放行→恢复拒绝且地址canary不变。

`0x0c709b68`/`0x0c707100`证明bond callback含第五个reason/status参数，
transport=1才是BREDR；LE绑定通知不能结束本模块的普通蓝牙配对窗口。
现在只把BREDR、BONDED=2且status=0视为成功，失败通知保留窗口。
`0x0c709c78`证明display type为0确认、1输入、2同意、3通知、4PIN；
仅0/2发送0x62，纯通知不再错误发送确认并显示失败。

点选手机后先请求ACL连接，再等待异步状态；IPC92查询返回bond枚举，不是status。
BONDING时不重发create-bond；NONE按800ms起步、3秒间隔最多6次尝试，不删密钥。
窗口内每秒检查可见性，系统临时改回connectable时重置discoverable，期限不延长。
配对页新增ACL/bond返回值、transport/state/reason、尝试次数和display type/passkey诊断。
生产状态机回归覆盖BLE通知/失败reason不误收尾、异步等待、纯通知不发送确认及可见性维持。
这些修正依据固件和主机回归；需用户实机确认手机实际配对过程。

## 蓝牙配对独立开关及 BTPAN 的 SPP 前置流程

“蓝牙配对”页首加入“启用蓝牙配对”，默认兼容旧版为开启，独立保存于
`/data/chaos/connect-radio.cfg`（`CCR1` + enabled u8）。关闭后立即禁止 SPP
读写、配对及扫描请求，并由各 owner 停止本模块扫描、收起配对窗口、恢复系统
原可见性和名单门禁、停止通道19服务、关闭pipe并断开其profile。不会关闭整机蓝牙
或删除已有绑定。异步写仍等 native 完成回调后才复用缓冲；快速关开也先完成清理。
重新开启清除旧错误并重新启动服务／配对窗口，注册客户端保留为无操作状态，
不重复注册同一个任务组。原插件载波仍由 ChaosConnect 总开关控制。

BTPAN 点选扫描结果或上次设备后，复用本普通蓝牙配对，不再直接发起旧的 PAN 配对：

1. 手环启用 BTPAN，扫描并点选手机；若配对开关已关闭，页面明确提示先开启。
2. 使用同一手机地址完成普通 BR/EDR 配对；不会删除密钥或反转地址反复配对。
3. 手机打开 Android ChaosConnect，连接手环的专用 SPP 服务。此处是实际 RFCOMM
   pipe read-start 成功，单纯 BONDED、SPP proxy 回调、其他手机的连接都不能放行。
4. 同地址SPP就绪后由 PAN worker 提交原 `network_connect_address`，随后仍走原生
   L2CAP/BNEP/DHCP。底层再次验证SPP，跳过重复 create-bond/remove-bond。

手机须开启系统蓝牙网络共享。SPP仅承担连接前置与原ChaosConnect数据，PAN数据仍走BNEP；
没有将网络数据塞进串口。等待上限90秒，扫描／断开／停用可取消尚未提交的PAN请求。
关闭配对开关取消等待中的准备；已经建立的PAN由BTPAN自己的开关／断开管理。
两条连接都只有一笔前置请求，其他手机的SPP会被拒绝。

验证增加真实生产C开关关开、残留回调拒绝、同地址SPP read-ready门禁、错地址、
取消及超时测试；`tools/test_pan_spp.py` 编译实际 `bt_probe_connect`／`bt_probe_poll`，
验证SPP未就绪或交接前断开时拒绝PAN，以及成功时不再创建／删除绑定。
另执行25项ChaosConnect回归、BTPAN UI回归、原ARM载波探针及完整构建。
这些是主机／固件指令验证，SPP与PAN在实体手机上的并存和联网需真机复测。

## 插件权限与 Android 权限分组

ChaosConnect → 权限设置只保留“Android端权限”和“插件权限”两个分组入口。
插件新增独立的“允许插件连接”（开启／关闭／询问），以及读取、写入、
删除和重命名、开发者模式、设备控制（预留）五个权限位，位定义与Android相同。
默认询问、只读。两端配置互不改写，读取／写入／开发者模式撤权会中止对应在途操作。

插件配置 `/data/chaos/connect-plugin.cfg` 为7字节：`CCL1`、连接策略u8、权限掩码u8、
`策略 XOR 掩码 XOR 0xa5`。临时文件 `.tmp`，完整写入并关闭后rename；
配置缺失／无效采用询问、只读，保存失败保持原值。新配置及临时文件也禁止远程改写。
旧 `CCP1` 开发者模式策略保持原值，两组都提供“开发者模式二次确认”入口，
明确标注这是共用的第二道门禁；单独打开一端的开发者模式位不会跳过它。

插件空HELLO格式与11字节应答不变；连接请求会在手环弹出“是否允许插件连接”。
确认前只接受HELLO、PING、GET_ACCESS和DISCONNECT，其余请求拒绝。
GET_ACCESS布局不变，kind=0表示插件，返回该插件的实际策略、掩码和连接确认状态；
PING的8字节格式继续表示旧开发者模式状态，不将连接确认混作开发者授权。
未接入GET_ACCESS的旧插件可能先显示“没有权限”，在手环确认连接后刷新目录即可；
上传还需在手环开启写入权限。此轮无需改插件的帧结构，但未声称旧插件会实时展示权限掩码。

双方的身份由桥接层物理来源决定，HELLO载荷不能自选另一端权限。
现有Android与插件各自通过已授权通道通信；设备控制位仍未实现任何opcode。

新增插件连接允许／拒绝／超时／旧token、伪造CCA1、文件权限独立与撤权、
配置保护、保存失败、重启加载及插件弹窗文案回归。共29项主机回归通过，
另执行ARM载波／提示框探针和完整3.101.043构建。实机插件确认与权限操作仍需复测。
