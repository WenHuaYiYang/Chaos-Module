# ChaosConnect 文件桥（p67 / 3.101.043）

当前文件桥使用独立的 **ChaosConnect v1**。旧 CPC2、ConnectListen/Send/Message、fake Canopus context、模块 ABI 和兼容实现已从 `supervisor/bridge/` 移除，旧文件管理插件不再兼容。两端需要一起更新；原 `bridge.cfg` 开关继续使用，默认关闭。

## 实际承载与 AstroBox 接口

```text
AstroBox v4 插件
  transport.send + register_transport_recv + transport-packet
    → 已认证的小米 SARv2 / L2 PB 通道
      → WearPacket.Type.OTHER = 0 / ChaosConnect namespace
        → ChaosConnect CCN1 二进制帧
          → Chaos 原生 NuttX VFS
```

这是新的私有应用协议，复用宿主已经建立的蓝牙会话。**没有新增独立的 Bluetooth L2CAP PSM，也不直接从 WASM 打开 L2CAP socket**；实际蓝牙承载由 AstroBox 选择。现有 v4 WIT 未开放裸 L2CAP API，不能把虚构的接口写进插件并宣称可用。

插件仅导入官方 `device`、`transport`、`register`、`dialog`、`timer`、`ui` 六组接口，权限为 `device`、`request`、`register_transport_recv`。官方文档的 transport 页仍描述早期占位状态；本次以最新官方宿主源码为准核实 send 的 protobuf 解码、SAR 入队和接收事件分发。宿主版本必须已实现这些接口；较旧的占位实现仍可能不能连接。

核对依据：

- [v4 概览](https://abox.run/docs/plugin-development/v4)、[WIT](https://abox.run/docs/plugin-development/v2/general/wit-files)、[订阅与权限](https://abox.run/docs/plugin-development/v2/host-api/register)。
- [宿主 v4 transport 实现](https://github.com/AstralSightStudios/AstroBox-NG-Module-PluginSystem/blob/main/src/v4/host/transport.rs)、[共享发送实现](https://github.com/AstralSightStudios/AstroBox-NG-Module-PluginSystem/blob/main/src/api/host/transport.rs)。
- [接收事件分发](https://github.com/AstralSightStudios/AstroBox-NG-Module-PluginSystem/blob/main/src/manager.rs)：`transport-packet` 的载荷是 **直接的标准 Base64 字符串**，不是 JSON 对象。
- [WearPacket schema](https://github.com/AstralSightStudios/AstroBox-NG-Module-Pb/blob/main/protos/xiaomi/wear.proto)、[MessageContent schema](https://github.com/AstralSightStudios/AstroBox-NG-Module-Pb/blob/main/protos/xiaomi/wear_thirdparty_app.proto)。

私有消息用已定义的 `thirdparty_app.message_content` bytes 字段承载，BasicInfo 名称为 `ChaosConnect`、fingerprint 为空，外层 type 使用 schema 已定义的 `OTHER=0`。不能使用未知 protobuf 字段藏数据，因为宿主会 decode/re-encode 并丢弃未知字段。插件测试通过宿主同版本 prost / pbjson 生成器验证二进制往返和 JSON 日志序列化，确认请求 ID 和所有二进制内容完整保留。宿主订阅的 type 过滤值 0 表示通配，插件须继续检查 type、包名、帧 CRC 和连接前缀。

## 固件核对与生命周期

| 项 | p67 / 3.101.043 证据 |
|---|---|
| 连接链表 | `0x200E16B8`；16B 节点 `+12` 才是 SAR connection |
| RX 处理入口 | connection `+12`，原值 `0x0C7D6E6D`；只替换原入口，不覆盖第三方 hook |
| handle / SAR 容量 | connection `+6` / `+0x52`，后者是 SAR 协商容量，不是原始 L2CAP MTU |
| 查会话 / 解密 | `0x0C7D7E95` / `0x0C7D46A5` |
| CTR 的 IV | `0x0C70262E..0x0C70263A` 把 IV 复制到栈上，不推进会话 IV；探测后原包可原样转交系统 |
| 发送 | `0x0C7D719D` 就地加密调用方缓冲；`0x0C7D6484` 将帧复制到 SAR 自有队列后返回 |
| SAR 帧限额 | `0x0C7D63F0..0x0C7D6404` 拒绝 `len+1 >= capacity` 或 `len > 0x7FFE` |
| owner / BT loop | `0x20125700` / `0x201259E4`；timer init/start `0x0C2E3995` / `0x0C2E3B85`，对象136B |

RX 识别 OTHER 类型、ChaosConnect namespace、加密写 opcode 2 的消息，仅将 CRC 正确且请求 ID 一致的请求入队；命中本命名空间的损坏帧丢弃。其他包名、普通 PB、其他通道（包括旧 `0x43`）原样交回系统。VFS 和协议分发在 owner loop；发送在 BT loop。TX 保留明文并在每次尝试前复制，避免发送失败后把密文再次加密。

队列有12个 RX 槽、16个 TX 槽；协商窗口8。处理请求前预留应答空间，进度事件不会挤掉应答。连接变化只接受新 HELLO；设备会清理打开的文件与未提交临时文件。停止时恢复 RX、递增 epoch，旧队列不能在重开后继续执行。文件清理交给 owner，不在设置页回调中执行。活跃维护为2–10ms，关闭后退到1s。

## 文件操作与传输

- 保留目录分页、STAT、创建目录、删除文件/空目录、重命名、设备内复制、上传、下载、文本预览/编辑、取消和开发者模式。
- 默认只写 `/data`；开发者模式开放其他实际路径，`/dev`、`/proc`、`/sys` 始终拒绝。开发者模式在新会话 HELLO 时清零。
- 数据块上限2048B，按 `min(2048, SAR_capacity-128)` 协商；8块在途，发送不逐块等待蓝牙 RPC。目录响应也遵守协商大小，分页保留连续顺序。
- 上传不超过32MiB，使用同目录独有临时文件、短写循环、偏移与 CRC32 校验，全部 WRITE 应答确认后才 COMMIT。rename 失败保留原文件，不用先删除原文件来伪造成功。
- 中断、CRC 失败、ABORT、新 HELLO 会清理未提交临时文件。临时后缀需要额外路径空间；过长目标明确报错。
- 插件串行消费接收事件，按偏移重排下载块，防止并发异步写本地文件时颠倒顺序。上传/下载等待应答15s超时会取消；传输中禁止切换到其他设备。

## 速度目标与验收

目标按标准小写单位解释：**200 kb/s = 25,000 B/s**；与 **200 KB/s** 不同。界面同时显示 KB/s 和 kb/s。

插件提供“链路测速（1 MiB）”：通过真实宿主设备链路发送8块窗口、逐字节核对设备回显，用单调时钟统计每方向的有效数据量/时间，不写手环文件；完成后显示是否达到200 kb/s。上传/下载另有真实文件操作的速度统计。回显测试只代表链路能力，不能代替闪存文件传输结果。

**本次没有真机无线测速，不能声称已达到200 kb/s。** 主机执行速度、队列容量或理论窗口带宽都不是蓝牙实测值。需安装两端产物，在实际手机/Windows/蓝牙适配器上各运行测速和大文件上传下载；链路协商、信号和宿主调度可能使速度低于目标。

## 构建与验证

```sh
python3 tools/test_chaos_connect.py
# 需要 Unicorn / pyelftools 与可用的 ARM clang、rust-lld：
RUSTUP_TOOLCHAIN=nightly python3 tools/probe_chaos_connect.py research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin
python3 tools/build.py
```

- 文件引擎测试调用生产 `chaos_files.c` 与 `chaos_connect.c`，VFS 替换为真实临时目录，覆盖大文件、短写、CRC、重命名失败、目录与协议边界。
- ARM 探针执行生产 transport/codec 与固件真实 CTR wrapper，AES primitive、radio、timer、文件引擎为明确替身；覆盖普通包、同为 OTHER 的其他包名和旧私有类型的转交、损坏私有帧拒绝、8帧窗口、发送重试、epoch 取消和第三方 hook 保护。
- 插件 `cargo test --lib` 验证实际 prost 往返与 pbjson 日志序列化、旧枚举错误复现、启动无设备/传输调用、协议边界、全部 ACK 后提交；`sh scripts/build_plugin.sh` 生成 WASI 组件和 `dist/chaos-connect.abp`。

安装时同时更新Chaos安装包和插件；先重启设备，再运行新版Chaos，释放旧驻留文件桥的入口。开启 Chaos 设置中的“文件桥”，插件启用后先“刷新设备”，再“连接ChaosConnect”。无需安装旧模块或启用旧兼容插件。

固定互通向量保存在`CHAOSCONNECT-V1.hex`（插件位于`plugin/tests/`）：第一行为HELLO请求CCN1帧（request_id=0x1234），第二行为同一帧的宿主protobuf承载（packet_id=0x12341234）。两仓库的向量逐字节相同，C与Rust测试分别核对。

## 插件启用失败：Invalid variant 17219 / SendError

首次故障发生于HELLO发送前的宿主日志序列化：旧外层 `0x4343` 是未知枚举，prost可以保留，pbjson却拒绝；宿主 `layer2.rs` 对结果unwrap，导致ECS线程panic。之后查询设备或再次启用插件显示的SendError是核心线程退出后的结果。

设备与插件0.2.1同步改用OTHER，不接受旧私有类型，命名空间/CRC保护保持不变。插件on_load不再自动发现设备和发包，首次手动刷新后才接收设备变化并更新列表。回归测试复现旧错误，并验证新HELLO、数据块和进度事件可通过宿主JSON日志路径。**必须完全退出并重启AstroBox**才能恢复已退出的核心线程，仅切换插件开关无效。
