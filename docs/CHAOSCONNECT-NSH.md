# ChaosConnect NSH 终端

目标：小米手环 10 Pro / 3.101.043。Android 0.5.0 的底栏为“主页、文件管理、终端、连接、我的”；连接页下方的协议、设置、关于移至“我的”，主题仅保留设置内入口。终端页在已连接且已获得 NSH 权限时自动打开，支持命令、Ctrl+C、停止/重启、清空和文本选择。主机按 UTF-8 流增量解码，处理常见 ANSI CSI/OSC、退格和回车，保留最多 32K 字符；这是命令行记录界面，不是全屏 VT100 终端。

## 手环授权

ChaosConnect → 权限设置 → Android端权限 / 插件权限，分别新增“NSH终端”。权限位 `0x20`，默认关闭；与读取、写入、删除、开发者模式及预留的设备控制位独立。允许后可执行原生系统命令，不受文件管理的 `/data` 范围限制。配置沿用 CCA1 / CCL1 七字节记录，合法 mask 从 `0x1f` 扩展到 `0x3f`；旧配置无需迁移，NSH 位保持关闭。

连接确认通过之前不能开终端。撤销 NSH/连接权限、停用 ChaosConnect、重新 HELLO、DISCONNECT 或心跳超时均撤销终端 lease，并异步停止 PTY。Android 与插件由实际载波身份选择各自权限，HELLO 内容不能选择身份。插件客户端尚未增加终端界面，但同一私有协议与手环权限检查适用于插件。

## 协议

原 CCN1 / WearPacket / SPP 承载保持不变。HELLO 的 capabilities 新增 `0x10` 表示 NSH 可用，GET_ACCESS 仍是 16 字节、mask 为 u32LE。

| 操作码 | 请求 | 成功应答 |
| --- | --- | --- |
| `0x50 NSH_OPEN` | 空 | `lease:u32LE + state:u8 + error:i32LE` |
| `0x51 NSH_WRITE` | `lease:u32LE + bytes` | 空 |
| `0x52 NSH_READ` | `lease:u32LE + capacity:u16LE` | 上述 9 字节状态头 + 原始输出 |
| `0x53 NSH_CLOSE` | `lease:u32LE` | 空 |

输入与请求的输出容量不得超过协商 chunk（最大 2048）；WRITE 拒绝空输入、NUL 和 EOT，支持 LF 和 Ctrl+C。READ 不阻塞，没有输出时仍返回状态头。状态：0 Idle、1 Starting、2 Running、3 Exited、4 Failed、5 Stopping。错误为负 errno。未经授权返回 ACCESS=3；旧 lease 返回 SEQUENCE=11；资源回收中返回 BUSY=8。一个会话的重复 OPEN 幂等，关闭/重连后的新 lease 不接受旧输入或旧关闭。输入不会在不确定结果后自动重发。

## 实现与所有权

`remote_nsh_engine.rs` 从现有网页 NSH 后端提取，不改 043 的已验证 PTY、spawn、dup2、waitpid 和 console 入口。`web_nsh.rs` 与 `connect_nsh.rs` 各自实例化完全独立的状态、SPSC 队列、PTY、shell 子任务和常驻 worker；本地 `nsh.rs` 也独立。

CCN1 owner 只调用非阻塞队列 API，BT 接收回调不执行 shell/VFS，LVGL 不接触终端 FD。停止先禁止输入、清除未写入 TX 和短写尾部，再发送 Ctrl+C、退格清理及 `exit\n`。CCN1 版本只在 waitpid 确认回收后允许新子任务，不遗弃未回收任务来伪造停止；如果系统命令拒绝退出，新 OPEN 返回 BUSY，直到原任务实际退出。网页后端保持原停止策略。

Android 在终端可见且前台时约每 150ms 读取，后台/其他页面每 1000ms 读取。断连立即停止本地轮询；手环按其 owner tick 执行撤权。原生 NSH 的执行能力来自被授予的独立权限，不需要开启文件协议的开发者模式。

## 验证

- `test_chaos_connect.py`：31 项生产 C / 权限回归，包括双客户端 NSH 默认拒绝、lease、碎片输出、旧 token、撤权/断开停止与关闭门禁。
- `test_connect_terminal.py`：独立 CCN1 PTY 引擎 27 项生命周期回归；网页后端相同的 27 项继续通过。
- 原 .043 `probe_nsh_spawn.py` 和 `probe_nsh_pty.py`：任务/FD 隔离、真实 ioctl/line editor/退出等接口探针，PTY 探针 11 项通过。
- Android JVM 覆盖 NSH 权限/状态/输入输出、UTF-8/ANSI、重启/撤权，并以生产 C 文件引擎和 Kotlin 协议互通；PTY 在互通测试中为显式 echo seam。
- 模拟器验证两套 UI、五项 Dock、原生/悬浮导航、设置持久化与连接确认；终端截图使用明确的 UI 测试输入，不代表实机输出。

以上不证明实体蓝牙链路、真实 shell 调度或真机流畅度。安装配套模块和 Android APK 后，需要手环允许 Android NSH 权限进行最终联调。
