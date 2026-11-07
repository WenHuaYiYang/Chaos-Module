# Chaos 网页服务器

适用目标：Xiaomi Smart Band 10 Pro（p67），固件 3.101.043。网页服务使用模块自己的 BTPAN/lwIP 网络接口，默认关闭，没有预设密码；不会因为打开页面就自动建立蓝牙连接或开放终端。

## 使用

1. 在共享网络的手机或电脑上开启蓝牙网络共享（NAP），然后进入 Chaos 主页的“btpan联网”，扫描并选择对应设备，完成必要的配对确认。
2. 等待 BTPAN 页面显示已获取 IPv4 地址。只有蓝牙连接、没有 DHCP 地址时，网页还不可访问。
3. 进入 Chaos 的“网页服务器”，用手环键盘设置 8～31 字节的可打印 ASCII 密码，用户名固定为 `chaos`。密码仅存在运行时 RAM，重新启动模块/设备后需重新设置。
4. 打开“网页服务器”开关。在可访问该 BTPAN 子网的浏览器输入页面显示的 `http://手环IP/`，使用手环上显示的用户名和密码登录。
5. 用完后关闭网页服务器。修改密码会关闭服务并撤销旧连接；需要手动重新开启。

可达性取决于共享端路由：共享手机/电脑通常是该 BTPAN 子网的网关，但同一 Wi-Fi 上的另一台设备不一定能访问它。手机的 NAT、网络隔离、防火墙或操作系统限制可能阻止转发。获取地址不等于公网可达；本功能不会创建公网转发、UPnP 或穿透。不要把 TCP 80 端口映射到互联网。

## 控制台功能

- 设备概览：目标型号/固件、实际 IP、掩码、网关、DHCP DNS、MTU、收发帧计数和可用的系统信息。缺失的运行时间、堆信息等为 `null`/“暂未提供”，不会用随机数代替。当前没有 CPU 占用、温度、文件系统容量测量、录音管理或日志下载。
- NSH：认证后启动独立的 PTY/NSH，不复用手环本地终端的收发队列。支持命令、历史、Tab 和 Ctrl-C 输入，采用文本输出而非完整 VT/ANSI 终端模拟器；全屏程序和复杂光标控制不保证正常显示。每次输入最多 1024 字节，NUL 和 Ctrl-D 不被支持。
- 网页应用：上传单个不超过 16 KiB 的 HTML，暂存在模块 RAM；点击运行后在浏览器内打开，不是在手环上执行原生应用。重启/卸载模块后清除。关闭 HTTP 或点击停止会阻止后续访问，但不会从另一台浏览器撤回已经下载的页面。
- 设备设置：浏览器主题、自动刷新间隔、字号、退出登录。重启按钮只在平台声明支持重启时启用；接受请求后，先提交 HTTP 响应，再延迟至少 1.5 秒，并等待 NSH 完成回收，回收超时则放弃重启。

页面的 CSS、脚本、SVG 图标都随服务一起提供，没有 CDN/远程字体依赖。只需能访问手环局域网，不要求浏览器能连接互联网。浏览器刷新后需重新登录；主题等显示偏好可保存在 localStorage，认证凭据不会写入 localStorage。

## 认证和安全边界

本功能是带设备命令执行能力的管理服务，应只在可信的个人 BTPAN 网络上临时开启。HTTP Basic 和 WebSocket 都没有 TLS；Base64 不是加密。共享端或能够观察链路的网络参与者可能看到密码和终端内容，请使用独立密码，不要复用重要账号密码。

首页的登录界面可匿名获取，所有状态/控制/应用接口要求 Basic 认证。WebSocket 握手不启动 NSH，第一条完整的二进制消息必须是 `AUTH ` 加 Base64 的 `chaos:密码`，认证通过后才能创建终端。凭据不放进 URL、查询参数或协议日志。

有 Origin 的请求必须与当前 Host 的 HTTP origin 一致；不发送跨域允许头。应用使用不带 `allow-same-origin` 的 `sandbox=allow-scripts` iframe，应用响应与本地预览均添加限制资源加载、连接、表单和 base URL 的 CSP。这阻止应用直接访问父页面的认证状态，不应把它当作任意恶意 HTML 的完整网络隔离器；只上传自己信任的 HTML。

关闭服务、修改密码、BTPAN 失去地址时，PAN worker 在下一轮轮询中关闭所有 TCP 客户端和监听器、擦除缓存的认证值，并异步停止 NSH。即使 UI 在两次轮询之间快速关闭/开启，原子 generation 仍会撤销旧会话。网页服务可在网络恢复后重新监听，但旧终端会话不会续接。旧 NSH 处于 STARTING/RUNNING/STOPPING 时，不允许创建新的 WebSocket NSH；回收未确认完成时不会复用环形队列/PTY。

浏览器“退出登录”清空本页的凭据并关闭本页终端，不是修改设备密码，也不会撤销其他已经登录的浏览器。若要撤销所有会话，请在手环上关闭服务或修改密码。

## 接口和上限

| 地址 | 方法 | 行为 |
| --- | --- | --- |
| `/`、`/index.html` | GET/HEAD | 内嵌 gzip 控制台 |
| `/api/system/status`、`/api/system/metrics`、`/status` | GET/HEAD | 系统/网络 JSON |
| `/ws/nsh` | WebSocket | 首条二进制 AUTH，随后二进制终端输入/输出 |
| `/api/nsh/exec` | POST | 原始命令体；确保结尾 LF，完整块一次入队 |
| `/api/nsh/output` | GET | 消费一段输出，返回 output/state；HEAD 不消费数据 |
| `/api/apps/upload` | POST | 单个 HTML 请求体，存 RAM |
| `/api/apps/run`、`/api/apps/stop` | POST | 发布/停止 RAM HTML |
| `/app/` | GET/HEAD | 认证后获取已发布的 HTML |
| `/api/system/reboot` | POST | 显式重启请求，平台不支持时返回 501 |

同时最多 3 个 TCP 客户端、1 个 NSH 会话。HTTP 与 WebSocket 终端互斥，忙时返回 409 或 WebSocket 1013。输入队列满时整条输入失败，不会留下半条命令；WebSocket 会明确关闭，HTTP 返回 503。上传新 HTML 时若旧 HTML 正在发送则返回 409，避免改写发送中的内存。

请求头最多 2048 字节、请求体最多 16384 字节；HTTP NSH 命令最多 1024 字节（服务端可追加一个 LF），WebSocket 完整消息最多 1024 字节。拒绝歧义 Content-Length、Transfer-Encoding、非法请求行、NUL/裸 CR-LF/折叠头；HTTP/1.1 要求唯一且非空的 Host。支持分片 TCP 接收和短 TCP 写入，不支持 HTTP keep-alive、chunked body、WebSocket 扩展/压缩。

未完成 HTTP 请求以及 WebSocket 认证期限为 8 秒，HTTP 响应发送期限为 60 秒。认证后的 WebSocket 120 秒未收到有效客户端帧会关闭；内置前端每 30 秒发送一个空二进制心跳。HTTP NSH 60 秒无提交/输出读取会停止，空闲输出探测不会占用 NSH。监听失败按 2 秒退避重试。计时使用可回绕的 32 位毫秒差值，显式状态位区分未启用计时器和时间值 0。

## 所有权与验证

`chaos_httpd_poll` 只由 PAN worker 调用。客户端、请求/响应/应用缓存、认证缓存、终端唯一占用指针只在该线程读写；UI 只通过原子的启用/generation 与诊断快照交互。平台凭据读取是同步快照，PTY 输入/输出经独立 NSH 的 SPSC 边界传递。HTTP 不调用固件地址，所有固件能力经 `chaos_web_platform.h` 的 C/Rust 接口提供。UI 快照字段独立原子读取，允许在切换中短暂显示跨轮询的统计组合，不作为管理锁使用。

执行：

```sh
python3 tools/embed_web.py
python3 tools/embed_web.py --check
python3 tools/test_web_server.py
```

`embed_web.py` 把 index.html 的唯一 console.js 标记替换为脚本，再生成无文件名、固定时间戳的 gzip C 头；`--check` 检测过期产物。协议测试使用 clang/宿主机 C11 编译器，以 `-Wall -Wextra -Werror` 直接编译生产 chaos_httpd.c、websocket_codec.c 和隔离的宿主机 transport/平台 harness。

2026-11-07：29 项测试通过，覆盖 gzip/HEAD、认证/Origin、严格 framing、分片请求、短 TCP I/O、三客户端拒绝、16 KiB HTML/CSP、原子 LF 提交、启动失败清理、只读输出探测、WS 握手/分片/掩码/控制帧/关闭码/UTF-8、STOPPING 拒复用、断网/开关/密码撤销、重启等待响应及回收、毫秒回绕和 JSON 转义上限。另检查前端所有直接引用的 DOM ID、独立资源、重复 ID 和 Node JavaScript 语法（系统有 Node 时）。

这些是宿主机协议与生命周期证据，不代表已经在真实 p67 设备验证蓝牙、NAP、DHCP、固件 PTY/NSH 或重启。发布前仍应在 3.101.043 实机按“BTPAN 获取地址 → 浏览器认证 → NSH 命令与断开 → 关闭/改密码撤销连接 → 网络恢复”的顺序验证，并观察空闲/连续使用时内存和线程回收。
