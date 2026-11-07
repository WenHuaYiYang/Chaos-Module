# 25 · 手机端实时控制手环屏幕：可行性判断

> 结论先行：**能实现，但要把它定位成“低帧率屏幕镜像 + 触控注入”，不是 PC 远控那种 30/60 fps。**
> 最现实的路线是 **Chaos 原生 screen 服务 + AstroBox 插件（或手机端网页）**；
> **“在手环里跑 Web 服务端”目前不建议**，详见第 5 节。

## 1. 结论

| 形态 | 可行性 | 预期体验 | 工作量 | 风险 | 建议 |
|---|---|---|---|---|---|---|
| AstroBox 插件 + Chaos bridge | ✅ 高 | 3–8 fps，150–400 ms | 3–6 周 MVP | 中高 | 最快验证 |
| **Chaos 制作台 App 增功能** | ✅ 高，但需补传输层 | 8–15 fps，100–250 ms | 6–10 周 | 中高 | **有 App 团队时首选** |
| 单独手机同步器（从零写） | ✅ 高，但重 | 8–15 fps，100–250 ms | 2–4 个月 | 高 | 不建议从零 |
| 网页 UI 由手机提供 | ✅ 高 | 与上面同，取决于渲染 | +1–2 周 | 中 | 推荐作为显示层 |
| 网页服务端跑在手环 | ⚠️ 当前不建议 | 不确定，可能更慢 | 数月且不一定通 | 很高 | 先做 5.4 的验证 |
| 快应用做镜像端 | ❌ 低 | — | — | 高 | 不适合 |

一句话：

- **看屏幕、点按、滑动、返回/主页**：可行。
- **全系统界面**（桌面、通知、设置、第三方 App、表盘）：大概率可行，因为抓的是 LVGL 显示层，不是某一个 App。
- **流畅动画/游戏级远控**：BLE 链路上不现实。
- **手环当 HTTP 服务器、手机浏览器直连**：网络层现在比早期好，但仍缺服务端和入站路径，暂不作为主路线。

## 2. 为什么判断“能”

### 2.1 屏幕内容是可以拿到的

设备是 336×480 AMOLED，界面是 LVGL v9。固件里有完整的 NuttX framebuffer 驱动路径：

- `/dev/fb0`
- `FBIOGET_VIDEOINFO` / `FBIOGET_PLANEINFO` / `FBIOPAN_DISPLAY` / `FBIO_WAITFORVSYNC`
- `lv_nuttx_uv_fb_init`，日志串 `must have two psram framebuffer`
- `mmap`、`fs_mmap.c`

这意味着最理想的情况是：**直接 `open("/dev/fb0")` + `mmap`，读当前显示的那块 PSRAM framebuffer**。336×480 RGB565 一帧 322,560 B；双缓冲时按 `pinfo.yoffset` 选当前可见缓冲即可。读 framebuffer 比重新渲染便宜得多。

如果 `/dev/fb0` 的读/映射在真机上不成立，还有一条已恢复符号的兜底路线：

| 能力 | 符号 / 地址 | 说明 |
|---|---|---|
| 控件树渲染成位图（截屏） | `p67_blur_snapshot_to_buffer` `0x0cac0879` | thunk 到 `lv_snapshot_take_to_draw_buf` |
| 创建绘图缓冲 | `p67_blur_draw_buf_create` `0x0c587f39` | 格式 18 = RGB565 |
| 销毁绘图缓冲 | `p67_blur_draw_buf_destroy` `0x0cac0db9` | 与 create 配对 |
| 等 GPU 画完 | `p67_blur_gpu_finish` `0x0c187bb1` | 抓之前要 fence |

证据见 [12-Canopus-地址手册.md](../../../docs/12-Canopus-地址手册.md) 的“手环截屏”一节，以及 [地址表.csv](../../../docs/地址表.csv) 的 `p67_blur_*` 行。注意这些是 **静态恢复、未真机调用** 的符号，第一次用必须有兜底。

### 2.2 触摸是可以注入的

固件有：

- 触摸设备节点 `/dev/input0`
- LVGL 驱动 `lv_nuttx_touchscreen_create`
- `lv_indev_create` / `lv_indev` 全局
- `/dev/ukeyboard`、`/dev/mouse0`、`lv_nuttx_uv_uinput_init`（uinput 主要是键盘）

直接在 `/dev/input0` 上 `write()` 不一定行：NuttX 的 touch upper-half 会把 write 转给 lower-half 的 `write` 回调，而 hynitron 的 lower-half 是否实现该回调没有证据。更稳的是**在 LVGL 层注入**：

1. 从 LVGL 全局结构拿到 `lv_indev_t *`（LVGL v9 布局里 `indev_ll` 在 `lv_global+0x4C`，`indev_active` 在 `+0x58`；`LV_GLOBAL = 0x2010_3174`，需真机校验）。
2. 备份 `indev->read_cb`（结构体偏移 `+0x04`）和 `driver_data`（`+0x24`）。
3. 换成自己的 read 回调：有远端事件就返回远端坐标/按下状态，没有就调用原回调。
4. 远端抬手后恢复默认。

这样不需要碰触摸驱动，也不会破坏物理触摸；LVGL 会照常处理点击、长按、滑动和滚动。风险是需要验证 `lv_indev_t` 的字段偏移和“读回调每 30 ms 被调一次”的节拍。

### 2.3 传输通道已经存在

Chaos 已经内置了 CanopusConnect 兼容端，AstroBox 插件能直接连：

- `bridge.c`：CPC2 / ConnectListen / ConnectSend / ConnectMessage，接管 L2 channel `0x43` 加密会话。
- `band_files_embed.c`：现有文件管理器插件的协议。
- 单帧二进制 payload **最多 480 B**（`bridge.c:507`）。
- 手机 → 手环的 `canopusconnect.send` 最多 3 个在途请求；每个请求等一个 BLE 往返。
- **手环 → 手机方向是 `ConnectMessage` 事件**，不是 `send` 请求，不受那 3 个在途槽位限制；视频流主要走这个方向。

现有文件桥已经证明了“AstroBox 插件 ↔ Chaos 模块 ↔ 加密 L2”整条链路可用，参考 [24-FILE-BRIDGE.md](24-FILE-BRIDGE.md)。屏幕流只需要在同一桥上再加一个 module id（例如 `screen`），并支持多 listener。

AstroBox 侧也有正式接口：

- [CanopusConnect WIT](https://github.com/AstralSightStudios/AstroBox-Plugin-WIT/blob/main/deps/astrobox-psys-host-v4.wit)：`send` / `listen` 的 binary payload 上限 480 B。
- [插件开发文档](https://abox.run/docs/plugin-development/v4)：Level 4 支持 JS/Python/Rust、HTTP server、应用内浏览器。
- [HTTP server](https://abox.run/docs/plugin-development/v4/host-api/http-server)：可起 `127.0.0.1` 端口，申请 `http-server.lan` 后可局域网访问。

### 2.4 网络现状：比早期文档好，但仍不是“手环服务器”

工作区里有两代结论：

- 根目录 [05-网络通路.md](../../../docs/05-网络通路.md) 记录的是 `/dev/tun` 不存在、`net_tun_open` 返回 `ENOENT`。
- 但 `projects/chaos/chaos-fetch/` 已经补上了 TUN 模块，且当前安装器把 fetch 固定为启用（`chaos_installer.lua` 里 `fetch_enabled = true`）。该模块注册 `bt-net` 网卡、做 DHCP、DNS 拦截/解析；源码注释里有真机读数 `netdev_ready=1`、`ip_configured=1`、`dhcp_state=6`，并提到 HTTP fetch 已经能通。

所以“手环完全没有 IP”已经过时。更准确的说法是：

- **出站 TCP/HTTP 已具备条件**（fetch 能走）。
- **固件里没有 HTTP server**（没有 httpd/web server 的实现痕迹）。
- **手机 → 手环的入站连接没有验证**：手机侧 NetProxy 是 App 内代理，可能只转发手环发起的流，不暴露一条可路由到 `bt-net` 的入站路径。
- 在模块里写 HTTP server 也不轻松：模块上下文不能随意碰 VFS，不能随便建线程，阻塞 socket 还可能拖死工作队列；`chaos-fetch` 自己就记录了 VFS 重入死锁和 hpwork 卡死的坑。

因此“手环跑服务端、手机浏览器直连”不是不可能，但它是**独立的研究课题**，不适合作为第一步。

## 3. 推荐架构

```text
手机侧（任选其一）
  AstroBox 插件（控制 + 低帧率预览）
  或 手机本地网页（canvas 渲染，插件/同步器提供 HTTP + 数据）
        │  canopusconnect.send / listen
        │  CPC2 / L2 0x43 / 加密 BLE
        ▼
chaos_sup.ko
  ├─ bridge.c            多 module listener、事件队列、流控
  ├─ screen_service.c    /dev/fb0 读取 / 差分 / RLE / 分片
  └─ input_service.c     LVGL indev read_cb 注入
        │
        ▼
  LVGL v9 / 336×480 framebuffer
```

### 3.1 手环侧

**采集**

- 首选 `/dev/fb0` + `mmap`，每 100–200 ms 读一次当前可见缓冲。
- 用 `FBIOPAN_DISPLAY` 的 `yoffset` 判断哪块是当前显示缓冲；用 `FBIO_WAITFORVSYNC` 或双缓冲快照降低撕裂。
- 兜底：`p67_blur_snapshot_to_buffer` + `p67_blur_draw_buf_create`，但每帧重渲染，成本高，只适合做“关键帧”。

**压缩**

- 16×16 或 32×16 分块，只发变化块。
- 每块先做 RGB565 → RGB332（或 16 色调色板），再 RLE。
- 首帧发关键帧；后续发增量块；手机侧定期 ACK，丢块重传。
- 提供两档：`清晰` 336×480、`流畅` 168×240（手机端 2× 放大）。
- 单事件 payload 控制在 472 B 以内（480 B 减 module id/头）。

**输入**

- 手环 → 手机：屏幕块、状态、心跳。
- 手机 → 手环：`tap(x,y)`、`down/move/up`、`key(back/home)`、`start/stop`、`request_keyframe`、`quality`。
- 输入事件走 `canopusconnect.send`，频率低，3 个在途足够。

**流控**

- 事件队列 32–64 帧；满了丢“非关键帧”，保关键帧。
- 手机每 N 帧回一个 ACK（带最后完整序号），手环按序号重传。
- 闲置 10–15 s 自动停流；屏幕熄灭/息屏时暂停。

### 3.2 手机侧

**Chaos 制作台 App（`chaos-bandpack`，推荐）**

已有基础：Kotlin / Jetpack Compose，字体与图标投递包制作、设备选择、工程保存、素材预览都齐了。
它现在的定位是**纯离线打包器**：Manifest 不申请网络/蓝牙，README 明确写“只做制作，不做传输”。
所以往它加屏幕控制，页面本身不是难点，**难点是补一条实时蓝牙传输**。

推荐加法：

- 新增 `Dest.SCREEN` 与 `ScreenScreen.kt`，底部胶囊多一个“屏幕”入口；
- 新增 `data/link/`：BLE 扫描/连接、Xiaomi 鉴权、L2 加密、CanopusConnect；
- 复用现有 Compose 组件（`BandScreen` 的外框、`StudioToolbar`）做画面容器；
- 输入用 Compose pointer 事件转成 `down/move/up`；
- 权限、隐私政策、首次连接说明同步更新（现在政策写的是“不联网、不申请蓝牙”）。

如果只是把 `.bin` 生成器扩成“屏幕控制”，而不引入 BLE 层，那它仍然不能实时控制手环——**必须有传输**。

**AstroBox 插件**

- 优点：鉴权、连接、L2 加密、权限模型、安装分发都已经有。
- 缺点：插件 UI 是 DOM-like（`IMAGE`/`DIV`/pointer 事件），不是 canvas；每帧用 data URL 更新图片，开销要实测。
- 适合：控制面板、连接状态、低帧率预览、参数设置。

**手机本地网页**

- 插件用 HTTP server 暴露 `GET /frame`、`POST /input`；或独立同步器起 HTTP/WebSocket。
- 浏览器用 canvas 解码 RGB332/RLE，渲染和输入都更自然。
- 这是“网页控制”的正确形态：**网页在手机/电脑上跑，服务端在手机上，不在手环上**。
- 如果要局域网内另一台设备控制，申请 `http-server.lan` 或独立同步器监听 LAN。

**独立同步器**

- 复用 AstroBox-NG 的 core/BLE/L2 代码（AGPL）或自己实现小米 L1/L2 鉴权。
- 可以绕过 AstroBox 插件的 480 B / 3 在途限制，用自定义分片和更高并发，帧率上限更高。
- 代价：连接、重连、鉴权 token、Android/iOS 后台限制都要自己处理。

## 4. 性能预算

先给结论：**瓶颈在 BLE，不在“能不能抓屏”。**

| 项目 | 数值 |
|---|---|
| 屏幕 | 336×480 = 161,280 px |
| RGB565 原始帧 | 322,560 B |
| RGB332 原始帧 | 161,280 B |
| 168×240 RGB332 | 40,320 B |
| 单事件有效载荷 | ≈472 B |
| 手机→手环 send | 最多 3 在途，每帧等 BLE 往返 |
| 手环→手机事件 | 无 3 槽限制，受 BLE/SAR/手机处理能力限制 |

粗略估算：

- 若 BLE 往返 50 ms，3 在途 ≈ 28.8 KB/s；100 ms 则 ≈ 14.4 KB/s（这是“手机发手环”的上限，不是视频方向）。
- 视频方向若能做到 30–100 个事件/s，即 14–48 KB/s。
- 流畅档 168×240 + RLE，单帧压到 3–8 KB，理论 2–10 fps。
- 清晰档 336×480 + 分块增量，交互时单次变化 1–5 KB，静止时接近 0；首次关键帧 20–60 KB，约 1–3 s。
- 触控往返：BLE 30–100 ms + LVGL 读取 30 ms + 手机渲染，约 100–300 ms。

这些是**量级估算**，不是实测承诺。第一步就应该用现有文件桥传 1 MB 文件测实际吞吐，再决定清晰档/流畅档。

## 5. 三种形态的取舍

### 5.1 AstroBox 插件（推荐先做）

适合：

- 快速验证“抓屏 + 触控”是否成立。
- 复用现有连接、鉴权、安装链路。
- 控制面板、设置、低帧率预览。

限制：

- 480 B/事件；手机→手环 3 在途。
- 插件 UI 不是 canvas，高帧率图片刷新可能成为瓶颈。
- 插件被停止/重载时端口和监听会释放；后台行为受宿主限制。
- 需要改 Chaos bridge：目前只支持 `band_files` 一个 listener，要加多 module 表。

### 5.2 Chaos 制作台 App 增功能（有 App 团队时首选）

仓库：[WenHuaYiYang/chaos-bandpack](https://github.com/WenHuaYiYang/chaos-bandpack)（AGPL-3.0，
Kotlin/Compose）。已经具备：

- 字体/图标投递包制作、设备选择（10 Pro / 9 Pro）、工程保存；
- `BandScreen` 等手环外观预览组件；
- 成熟的 MD3 Expressive 页面与导航；
- 对设备素材、包格式、签名/构建流程的完整校验。

但必须注意它**目前没有实时传输**：

- `AndroidManifest.xml` 没有网络/蓝牙权限；
- README 明确“只做制作，不做传输”，导出的 `.bin` 由官方 App 或社区工具侧载；
- 源码里没有 BLE/GATT、Xiaomi 鉴权、L2、CanopusConnect 的实现。

所以新增“屏幕控制”的真实工作量分两块：

| 工作 | 内容 | 估算 |
|---|---|---|
| **传输层** | Android BLE 扫描/连接、服务发现、通知、Xiaomi L1/L2 鉴权、SAR、CPC2/CanopusConnect | **3–6 周** |
| **屏幕页** | 画面解码/渲染、指针事件、连接状态、画质/帧率设置、断线重连 | 1–2 周 |
| **手环侧** | `screen_service.c` / `input_service.c` / bridge 多 module | 3–6 周 |
| **合规** | 权限、隐私政策、屏幕内容提示、首次授权 | 2–4 天 |

传输层有两条子路线：

1. **接 AstroBox 的协议核心**（推荐）
   - `corelib` 已有 Xiaomi 鉴权、SAR、L2、`canopus::connect`；
   - Android 侧仍需提供 BLE 收发的 sender/回调，或者把 AstroBox-NG 的 BLE 模块也接进来；
   - 与 Chaos bridge 现成的 `ConnectListen/Send/Message` 完全对齐，手环侧改动最小。
2. **在 Kotlin 里自己实现 Xiaomi BLE 协议**
   - 可参考 AstroBox-NG 的 Rust 实现、Tatum 的 C# 移植；
   - 需要自己管理鉴权 key、L1 分包/ACK、L2 AES、SAR 窗口；
   - 工作量更大，但依赖少、包体小。

两条路都绕不开一个产品问题：**Xiaomi 鉴权 key 从哪来、怎么保管**。`chaos-bandpack` 目前没有
导入/保管 key 的流程；AstroBox 已经把这套流程做完了。如果不想让用户手工导入 key，就只能走
XMS Interconnect（见下），但那不是 CanopusConnect。

**关于 XMS Wearable SDK**：仓库所有者的另一个 App `Snapnotes-andriod` 用了
`xms-wearable-lib` 的 Interconnect（`NodeApi` / `AuthApi` / `MessageApi`），鉴权由小米运动健康代办，
看起来更省事。但它是 **Interconnect，不是 CanopusConnect**：

- 手环侧要另做一个 Interconnect 监听端，和现有 Chaos bridge 不兼容；
- 消息是 JSON/byte[]，通道更适合通知、文件分块，不适合高频屏幕帧；
- 仍然需要手环端能抓屏、注入触摸。

因此不建议把 XMS Interconnect 当作屏幕流的主通道；它可以作为“控制指令”的备选，但视频仍建议走
CanopusConnect 事件方向。

**结论**：如果 App 团队愿意补 BLE/CanopusConnect，这个 App 是最佳宿主——UI、设备选择、工程、
素材管线都能复用；否则先做 AstroBox 插件验证，再回头接 App。

### 5.3 网页服务端跑在手环（暂不建议）

现在具备的条件：

- TUN 模块让手环有了 `bt-net` 和 IP，出站 TCP 可用。
- 有 `/dev/fb0`，可以抓屏。
- 有 `/dev/input0`/LVGL，可以注入。

还缺的关键条件：

1. **没有 HTTP server**：固件没有 httpd；要在模块里写，就得面对 VFS 重入、线程/工作队列、阻塞 socket 等约束。
2. **入站路径未验证**：手机侧 NetProxy 是否允许手机主动连到手环的 `bt-net` IP，未知。
3. **性能不占优**：HTTP + TCP over BLE 的头和重传，不会比直接走 CanopusConnect 事件更快。
4. **功耗/稳定性**：手环持续亮屏 + BLE + 压缩，续航会从“按天”掉到“按小时”。

判断：**如果“网页”指浏览器里的控制台，建议把服务端放在手机，手环只做采集/注入；如果坚持服务端在手环，先做 5.4 的验证，不要直接投入 UI。**

### 5.4 手环服务端的 go/no-go 验证

在做任何服务端代码前，先回答这三个问题：

1. 手环 `bt-net` 的 IP、掩码、网关是什么？手机能否 `ping` 通？
2. 手机能否主动 `connect()` 到手环上临时监听的一个 TCP 端口？（可以先用 `chaos-fetch` 的 socket 自检/临时模块做）
3. 手环模块里能否在不阻塞 UI/网络工作队列的前提下 accept/read？（现有 `chaos-fetch` 的经验是：socket 放错队列会卡死整机）

任何一条不成立，“手环跑 Web 服务端”就不成立。

## 6. 实施路线

### 阶段 0：可行性 spike（3–5 天）

- 在 Chaos 模块里 `open("/dev/fb0")`、`mmap`，把一帧 RGB565 写到 `/data/frame.raw`，重启后取回验证。
- 若失败，改用 `p67_blur_snapshot_to_buffer`。
- 目标：**证明能稳定拿到 336×480 当前画面**。

### 阶段 1：只读镜像（1–2 周）

- 新增 `screen` module id；手机侧用 AstroBox 插件或最小 Python/Rust 工具 `listen`。
- 首版不压缩，分块传 RGB332，1 fps。
- 验证事件方向吞吐、丢包、重连。

### 阶段 2：触控注入（1–2 周）

- 找到 `lv_indev_t *`，包装 `read_cb`。
- 实现 tap / down-move-up / back / home。
- 验证物理触摸与远端触摸不打架。

### 阶段 3：可用 MVP（2–4 周）

- 分块差分 + RLE + 关键帧 + ACK/重传。
- 清晰/流畅两档；息屏暂停；自动断流。
- AstroBox 插件控制页；或手机本地网页 canvas。

### 阶段 4：性能优化（按需）

- 独立同步器/自定义分片，绕过 480 B 与 3 在途限制。
- 局域网网页、键鼠映射、录制。
- 若阶段 0 的 `/dev/fb0` 路线稳定，这一阶段主要是传输优化。

## 7. 风险清单

| 风险 | 影响 | 缓解 |
|---|---|---|
| 固件地址硬编码 | 换固件即失效 | 只支持 3.101.043；启动校验镜像指纹 |
| `/dev/fb0` 格式/双缓冲与预期不符 | 抓屏花屏/撕裂 | 先 dump 验证；用 `FBIOGET_PLANEINFO`；vsync |
| GPU 还在画就抓 | 残帧 | `p67_blur_gpu_finish` / `FBIO_WAITFORVSYNC` |
| `lv_indev_t` 偏移不对 | 注入无效或崩溃 | 真机小步验证；先只读指针 |
| 单核、无 MMU | 越界即 HardFault/重启 | 所有指针先做范围/形状校验；静态缓冲 |
| BLE SAR 被流打满 | 通知/文件传输超时 | 限速、空闲停流、关键帧优先 |
| 插件 480 B/3 在途 | 帧率不够 | 视频走事件方向；必要时独立 App |
| 配套 App 目前无 BLE/鉴权 | 不能直接控制手环 | 先补传输层，或先用 AstroBox 插件验证 |
| 手机后台被挂起 | 流中断 | 前台服务/常驻通知；插件内提示 |
| 续航 | 连续使用仅数小时 | 会话制、自动息流、亮度策略 |
| 隐私 | 屏幕内容可被同会话看到 | 显式开关、手环端指示、超时断开、权限确认 |

## 8. 最终建议

1. **先做阶段 0**：`/dev/fb0` 能不能稳定读出一帧。这是整个项目的 go/no-go，成本最低。
2. **有 App 团队就优先加在 `chaos-bandpack`**：UI、设备选择、工程、素材管线都能复用；但先确认
   传输层是接 AstroBox 的 `corelib`，还是自己写 BLE/CanopusConnect。传输层是成败点，不是页面。
3. **没有传输层就先做 AstroBox 插件**：复用已经跑通的加密 L2/文件桥，最快看到“手机上看手环、点手环”。
4. **显示层优先用手机本地网页/canvas**，插件/App 负责连接和控制；DOM 插件只做设置和低帧率预览。
5. **不要先做“手环 HTTP 服务器”**。TUN 让网络条件变好了，但服务端、入站、线程模型三座山都还在；
   等阶段 0–3 有结论后再评估。
6. **对外表述要克制**：这是“可用的低帧率远控/演示”，不是“PC 级远程桌面”。能做到 5–10 fps、
   100–300 ms，已经可以完成看通知、改设置、点应用、简单回复；不要承诺 30/60 fps。

## 9. 相关文档

- [12-Canopus-地址手册.md](../../../docs/12-Canopus-地址手册.md)：截屏、GPU fence、draw buf 地址。
- [地址表.csv](../../../docs/地址表.csv)：`p67_blur_*`、`lv_display_*`、`lv_obj_tree_walk`。
- [24-FILE-BRIDGE.md](24-FILE-BRIDGE.md)：现有 CanopusConnect/L2 桥。
- [ARCHITECTURE.md](ARCHITECTURE.md)：Chaos 模块、定时器、页面模型。
- [LVGL-MAP.md](LVGL-MAP.md)：LVGL 类与函数地址。
- [chaos-fetch/README.md](../chaos-fetch/README.md)：TUN 模块约束。
- [chaos-bandpack](https://github.com/WenHuaYiYang/chaos-bandpack)：配套 Android 制作台（当前只做包，不做传输）。
- [chaos-bandpack AndroidManifest](https://github.com/WenHuaYiYang/chaos-bandpack/blob/master/app/src/main/AndroidManifest.xml)：确认当前无蓝牙/网络权限。
- [chaos-bandpack ChaosApp.kt](https://github.com/WenHuaYiYang/chaos-bandpack/blob/master/app/src/main/kotlin/com/chaos/bandpack/ui/ChaosApp.kt)：`Dest` 路由与 `NavCapsule`，新增屏幕页的入口。
- [05-网络通路.md](../../../docs/05-网络通路.md)：早期 `/dev/tun` 结论（已被 fetch 模块部分更新）。
- AstroBox：[CanopusConnect WIT](https://github.com/AstralSightStudios/AstroBox-Plugin-WIT/blob/main/deps/astrobox-psys-host-v4.wit)、[插件开发](https://abox.run/docs/plugin-development/v4)、[HTTP server](https://abox.run/docs/plugin-development/v4/host-api/http-server)。
