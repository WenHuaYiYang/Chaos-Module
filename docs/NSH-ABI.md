# p67 / 3.101.043 NSH 终端与 ABI 核对

只适用于小米手环 10 Pro（p67）固件 **3.101.043**。实现是 PTY 连接的持久交互式原生 NSH，不再是 `system()` 单命令执行后端。所有新入口来自这份 AP 的独立恢复；**尚未真机验证**。

## 核对对象和资料来源

- AP：`research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin`
- SHA-256：`519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec`
- 指令 VA = `0x0C0C0000 + file_offset`，数据别名 = `0x2C0C0000 + file_offset`。Thumb 可调用入口必须设置 bit 0。
- 用户提供的 `MIBAND10PRO-AUTORUN/modules/Canopus-Module/targets/xiaomi-band-10-pro-3.101.043/target.toml` 使用相同 SHA-256、Cortex-M33、ARMv8-M Main、soft float。该 target pack 的 `errno_location` 记录与本实现的 `0x0C1E45BD` 一致，仍标注 device=not_probed。
- target pack 的 `pthread_create_internal` 是 **FORBIDDEN / REJECTED**，本实现不用它，也不修改 target pack 的审核状态。
- `~/Other2/Vela_NativeApp-syspatch-band-11` 只作为架构参考。其 `0x0C3754B0`、`0x0C8C2DBC`、`0x0C350E8C` 等地址**禁止复制**到 p67；没有把跨版本匹配当成 ABI 证明。

## 已核对的调用链

| 用途 | Thumb 入口 | 合约 |
| --- | --- | --- |
| task_spawn | `0x0C209771` | 六参 `(name, entry, actions, attr, argv, envp)`，返回 pid 或负 errno；独立 task group/fd table |
| spawn attr init | `0x0C1EA725` | 20 字节，4 字节对齐；flags@0、priority@1、stack size@0xC、stackaddr@0x10 |
| flags / priority / stack / destroy | `0x0C1EA781` / `0x0C1EA7A1` / `0x0C1EA7C5` / `0x0C1EA7E1` | priority 参数为指针；使用 flags=4、priority=100、stack=32768，stackaddr 保持 NULL |
| file_actions init / adddup2 / destroy | `0x0C1EA711` / `0x0C1EA63D` / `0x0C1EA6E5` | 链头容器为一个指针；同用户堆分配/释放；只重映射 child fd |
| console constructor | `0x0C238E29` | 新分配的 0x5B8 字节实例，exit callback 位于实例 +0x2C |
| nsh_session | `0x0C23DDAD` | `(console, print_banner, argc, argv)`；argv=NULL 交给 spawn 自动产生 argc=1 |
| waitpid | `0x0C2063C5` | **WNOHANG=2**，返回 pid / 0 / -1+errno |
| usleep | `0x0C1F5D0D` | 微秒；工作循环退避 20 ms |
| errno / user free | `0x0C1E45BD` / `0x0C1F8FF9` | 当前任务 TLS errno；只对确定归本模块所有且 child 已退出的遗留 console 配对释放 |

stock `system()` 的 `0x0C240399` 入口和 `nsh_script` / `nsh_parse` 提供了恢复调用链的起点；生产交互后端并不调用 `system()`。

详细反汇编调用点、参数恢复与可重复指令执行证据：

- [NSH-SPAWN.md](NSH-SPAWN.md)：属性、argv、任务隔离、动作链、console 所有权、非阻塞 waitpid。
- [NSH-PTY.md](NSH-PTY.md)：PTY fops、ioctl 分派、非阻塞标志、前台 PID 和 Ctrl-C/Ctrl-D。

关键 PTY 规则：master 用 `/dev/ptmx` 打开，`0x403` **不是非阻塞标志**；FIONBIO=`0x30A` 接收 int 指针，只对 master 调用。解锁=`0x12C` 接收整数 0；查询编号=`0x12B` 接收 32 位 int 指针。slave 始终保持阻塞，不预占前台 PID，由 stock NSH 的 console ioctl 维护。

## Chaos 实现与生命周期

主页入口为“NSH终端”（page 31，总页数 32）。点击“开始/重开”请求常驻 worker 建立一次 PTY + shell；UI 仅通过静态有界队列发送/接收，不在 UI 线程 read/write/waitpid，不改 UI 任务标准输入输出，也不从 worker 操作 LVGL。

- worker 和 shell 通过已核对的 task_spawn 创建，不借用未核对的 pthread ABI。worker 的生命周期与已加载的 Chaos 模块一致；启动后空闲时退避，不为每条命令反复创建 worker。**不支持启动终端后的模块热卸载**。
- TX 使用 Acquire/Release 发布完整命令；短写、EINTR、EAGAIN 保留尚未写出的字节，队列满时失败并保留键盘输入。RX 有界，溢出会提示，避免持续输出阻塞退出清理。
- 原生 exit callback 负责释放 console 并结束 child；wrapper 只发布状态并委托原回调。worker 用 WNOHANG 轮询回收，不以“done 标记已写”为已退出依据。未知等待错误时不启动第二个会话，也不释放仍可能被 child 使用的对象。
- 返回主页、打开键盘或销毁终端页不会终止 shell；现有 UI 维护定时器在前台页分发之前排空 RX，保留有界历史。
- 支持输入命令、Ctrl-C、发送 `exit`、清屏、历史前后翻页和最新输出。NSH 输入默认小写，最长 255 字节；界面不是全屏 VT 模拟器，不支持全屏编辑器。
- p67 的 Ctrl-D 是被 readline 过滤的普通数据，不是 EOF。因此界面显示“发送 exit”，实际发送 `exit\n`；长命令执行中，该操作要等待 shell 重新读命令。

## 回归验证

从仓库根目录执行（原始 AP 不随发行产物打包）：

```sh
python3 tools/verify_nsh_abi.py
python3 tools/probe_nsh_pty.py --static-only
.venv/bin/python tools/probe_nsh_pty.py
.venv/bin/python tools/probe_nsh_spawn.py
python3 tools/test_nsh.py
python3 tools/test_terminal_ui.py
sh tools/test_keyboard.sh
python3 tools/build.py
```

主机测试执行生产 Rust 队列/后端/UI 代码；固件探针执行精确 AP 的 Thumb 指令，但底层 heap、VFS 叶子、锁、TLS、调度/激活和最终 task exit 为显式模型，**不是设备运行结果**。

真机验收仍需：启动后提示符，`help` / `pwd` / `echo` / `ls`，分包和长输出，键盘往返与后台输出，Ctrl-C，`exit` 后重连，低内存失败路径，屏幕休眠/唤醒，长时间调度、功耗和崩溃稳定性。未验证 GPU/cache、物理触摸和真实信号时序；不得凭编译通过宣称设备级稳定可用。
