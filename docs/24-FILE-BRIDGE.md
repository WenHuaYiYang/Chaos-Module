# 24 · Chaos 原生文件桥（替代 Canopus 模块）

## 目标

让现有的 AstroBox 插件 `手环文件管理器`（模块 id 仍为 `band_files`）直接连接 Chaos，
不再需要：

- Canopus 模块管理器
- Canopus Supervisor
- `band_files` Canopus 模块

插件端协议、UI 和交互保持不变。手环侧由 `chaos_sup.ko` 内置一个最小的
CanopusConnect 兼容端，文件操作直接走 Chaos 已验证的 NuttX VFS 封装。

```text
AstroBox 插件
  └─ canopusconnect（CPC2 / L2 0x43，原有加密会话）
       └─ chaos_sup.ko
            ├─ bridge.c                CPC2 + Connect + L2
            ├─ band_files_embed.c      原 band_files 协议与文件操作
            └─ fw_api / NuttX VFS
```

## 为什么不是直接搬模块

`band_files` 原本依赖 Canopus 的：

- CMR1 descriptor 注册与 `/dev/canopus`
- `ctx->fs` / `ctx->connect` 服务对象
- Canopus Supervisor 的 L2 0x43 加密通道

Chaos 没有这些运行时。本实现保留 `band_files` 的**文件协议**，替换掉
注册、服务对象和传输层：

- `band_files_embed.c`：原模块文件协议，跑在一个进程内 fake `canopus_context_v1` 上
- `bridge.c`：实现 ConnectListen / ConnectSend / ConnectMessage，直接接管 L2 0x43
- 文件服务：`fw_api` 等价 NuttX 入口，不经过 Canopus

## 工作方式

1. Chaos 维护定时器 `chaos_shake_timer` 在 UI owner 就绪后调用 `chaos_bridge_start()`；不在 `chaos_ctor` 里启动，避免 LVGL timer 源尚未就绪。
2. 桥用 LVGL/owner timer 引导出常驻的 10 ms poll（与 Canopus L2 相同的 owner-loop 方案）。
3. 手机建立加密 L2 会话后：
   - `ConnectListen("band_files")`：登记监听
   - `ConnectSend("band_files", 二进制帧)`：进入原 `band_files` 协议
   - 每个响应通过 `ConnectMessage` 事件推回手机
4. L2 0x43 只接受加密写（opcode 2），其他 channel 原样交还固件 handler。

桥不包含 Canopus 的模块 registry、descriptor、安装器、Manager RPC 或 native app。

## 文件策略

与 `band_files` 0.1.6 一致：

| 项 | 规则 |
|---|---|
| 默认写 | `/data`，但 `/data/canopus` 除外 |
| 开发者模式 | 其它绝对路径，排除 `/dev`、`/proc`、`/sys`、`/data/canopus`、`/canopus` |
| 分块 | 448 B |
| 单帧 payload | 最大 476 B（480 B 帧减 4 B 头） |
| 上传 | 临时文件 + CRC + rename |
| 开发者模式持久化 | **不持久化**，每次启动默认关闭；插件里重新开启 |

## 生命周期

| 事件 | 行为 |
|---|---|
| Chaos 维护定时器 | 按开关状态在 UI owner 就绪后启动/停止 bridge；失败只写 `BRIDGE_STATE=0xffff_ffff`，不影响 Chaos 页面 |
| 设置页开关 | 写 `/data/chaos/bridge.cfg`（`BRG1` + enabled），清 `BRIDGE_STATE`，下一次维护 tick 落地 start/stop |
| 关闭桥 | `chaos_bridge_stop()` 恢复原 L2 0x43 handler、清空队列，timer 降到 1 Hz 空转 |
| 重新打开 | 若 handler 仍是固件原入口则重新挂接；若 Canopus 已占用则不强行抢回 |
| 手机连接 | L2 会话建立后首个加密请求触发 `g_connected` |
| 手机断开 | 下一次会话句柄变化时清空 listener |
| Chaos 卸载 | 当前安装器从不卸载；bridge 不提供反向注册 |

`BRIDGE_STATE` 位于 `state.rs`，0 = 关闭/待应用，1 = 已启动，0xffff_ffff = 启动失败。

## 安装与使用

1. 安装 `dist/chaos-installer-p67-bridge.bin`（bridge 包）并重启；`dist/chaos-installer-p67.bin` 是不含 bridge 的 safe 包。
2. 默认文件桥开启。要和 Canopus 同时使用时，进 Chaos 设置页关掉「文件桥」，再在 Canopus 管理器里启用 Supervisor/`band_files`；反过来也一样，同一个 L2 0x43 只能有一个处理端。
3. 打开 AstroBox 的 `手环文件管理器` 插件（0.1.5），连接手环。
4. 状态应显示 `已连接 band_files（协议 v1）`，无需 Canopus 管理器。
5. 需要访问 `/data` 以外路径时，在插件里打开开发者模式；重启后默认关闭。

> 如果 Canopus Supervisor 仍在运行，它会和 bridge 抢 L2 0x43，表现为随机超时；
> 这不是 bridge 的稳定性问题，先完全停用 Canopus。

## 构建

```bash
sh tools/build_ko.sh
```

构建脚本会：

1. 用与 Rust 相同的软浮点 ABI 编译 `supervisor/bridge/{canopus_protocol,band_files_embed,bridge}.c`；
2. 以 `-DBF_BRIDGE` 编入文件协议；
3. 与 `libchaos_sup.a`、`corona_blur.o` 一起 `rust-lld -r`；
4. 走原有的 `fix_ko_layout`、`ko_slim`、`verify_chaos_ko`。

本地没有 clang 时会回退到 workspace 的 xpack `arm-none-eabi-gcc`；CI 仍用 clang。

## 体积

以当前源码实测：

| 产物 | 大小 |
|---|---:|
| 不含 bridge 的基线 | 288,786 B |
| 含 bridge | 306,132 B |
| 增量 | **+17,346 B（约 17.4 KB）** |

分段增量：`.text +9,712 B`、`.rodata +499 B`、`.bss +7,768 B`；其余是符号表/重定位。

bridge 三个目标文件的 `.text` 合计约 10.4 KB，链接 GC 后实际 `.text` 增加 9.7 KB；
`.bss` 增加 7.6 KB（L2 接收/发送环）。不复制 Canopus 的 registry/UI，因此远小于整套 Supervisor。

## 兼容性

- 仅 3.101.043。L2 地址来自 `targets/xiaomi-band-10-pro-3.101.043`。
- 插件端无需改动；模块 id 仍为 `band_files`。
- 不再需要 `/data/canopus`、Canopus 管理器或 `band_files.canopus`。
- AstroBox 的 `learn_window()` 查询不受支持，插件会按默认 4 个 RPC 槽工作；
  不影响文件管理器的一次一请求协议。

## 已知限制

- L2 回调替换是 SRAM 层，和 Canopus Supervisor 使用同一条固件路径；真机需要实测。
- 不实现 Manager 的 QueryDevice/QueryModule；只有 CanopusConnect 子集。
- 桥常驻一个 10 ms owner timer；空闲时只做 attach/队列检查，功耗影响需要实测。
- 文件操作在 UI owner 任务同步执行；大目录/大文件分块会占用该任务，和原模块行为一致。
- 没有 Canopus 的 descriptor/CMR1 注册，因此不能再用 Canopus 管理器启用/禁用本桥；
  它随 Chaos 模块加载。

## 验证清单

本地：

- `tools/build_ko.sh` 成功，`verify_chaos_ko` 未定义符号 0；
- `readelf -r` 只出现已支持的 ARM 重定位；
- `nm` 能看到 `chaos_bridge_start` 且 `chaos_ctor` 有调用。

真机：

1. 卸载/停用 Canopus 管理器与 `band_files` 模块；
2. 安装新的 Chaos 安装器并重启；
3. 打开 AstroBox 文件管理器插件，确认版本 0.1.1；
4. 连接手环，应显示“已连接 band_files（协议 v1）”；
5. 浏览 `/data`、上传、下载、重命名、删除；
6. 开开发者模式访问 `/data` 外路径；
7. 重启后确认开发者模式回到关闭（按设计）。

## 对 AstroBox 其它功能的影响

- bridge 只接管 L2 channel `0x43`；PB/文件传输等其它 channel 会原样交还固件 handler。
- 正常工作时，AstroBox 安装普通 `.bin`（表盘/第三方应用容器）走的是它自己的传输/安装通道，
  不经过 `band_files` 协议，因此不会被文件桥“当成文件操作”拦截。
- 但 SAR/加密 L2 是**整条蓝牙链路共享**的。如果 bridge 破坏了 SAR 状态，
  AstroBox 的 `.bin` 安装、设备连接、文件传输都会一起受影响；这不是 `.bin` 安装被协议拦截，
  而是底层蓝牙通道被 bridge 弄坏了。
- 判断方法：重启手环后不打开文件管理器插件，直接试 AstroBox 安装普通 `.bin`。
  能装 → bridge 没有全局破坏；仍超时 → 先装 safe 包恢复。

## SAR 风险与回退

bridge 会替换 SAR 的 0x43 RX 回调，并由 BT owner loop 发送响应。若手机日志出现：

```text
SAR retransmission limit exceeded: device stopped acknowledging (timed out)
```

说明设备端的 L2/SAR 已被打断，先按以下顺序恢复：

1. 重启手环，手机蓝牙关闭再打开，重新连接；
2. 确认 Canopus Supervisor / `band_files` 已完全停用（不能和 bridge 同时占用 0x43）；
3. 仍不能连接时，安装 `dist/chaos-installer-p67.bin` 或 `dist/chaos-installer-p67-safe.bin`（均不含 bridge）并重启。

调试 build 不应在 SAR 已不稳定时反复安装。

## 故障排查

| 现象 | 检查 |
|---|---|
| 插件一直“等待模块” | Chaos 是否加载；`BRIDGE_STATE` 是否为 1 |
| 连接后无响应 | L2 0x43 是否被其他 Canopus Supervisor 占用；先完全停用 Canopus |
| 目录为空/解析失败 | 确认插件为 0.1.1（u8 名称长度 + 476 B 分块） |
| 上传失败 | 目标目录是否在默认 `/data` 策略内；开发者模式是否开启 |
| 启动即异常 | 回退到不含 bridge 的 Chaos 安装器；bridge 启动失败不会注册页面之外的资源 |
