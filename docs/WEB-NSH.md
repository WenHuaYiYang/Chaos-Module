# Web NSH 与系统状态（p67 / 3.101.043）

此后端服务于已认证的网页控制台。它拥有独立的静态 RX/TX 队列、PTY、常驻 worker 与 child shell，不借用本地 NSH 页的会话或队列。网页关闭、断网或更换密码只停止网页会话，不更改本地终端行为。

## 精确固件证据

目标 AP 为 `research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin`，SHA-256：`519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec`。以下地址只适用于这一镜像；函数指针必须为奇数 Thumb callable。

| 用途 | Thumb callable / 证据 | 合约 |
| --- | --- | --- |
| CLOCK_MONOTONIC | `0x0C201FE1`；dispatcher 的 `0x0C202012` 调 `0x0C202194` | `clock_gettime(1, timespec*)`；`sec:i64@0`、`nsec:i32@8`，缓冲按 8 字节对齐 |
| monotonic tick leaf | `0x0C1AE158` | 内部输出 64 位 10 kHz ticks；真实转换指令写 seconds 和 nanoseconds |
| NSH system | `0x0C240399` | 固定调用 `system("reboot")`，内部 spawn `system` 子任务，argv 为 `-c, command, NULL`，随后阻塞 waitpid |
| reboot 命令表 | `0x0CCDB648` 的命令字符串指针 `0x2CB7EA58`、处理入口 `0x0C23D7D5` | 仅核对命令表/处理程序；主机探针不执行 boardctl 或重启 |
| meminfo 文件 | `0x0C23C6D4` 指向 `0x2CB2E2C4`，内容 `/proc/meminfo` | 沿用已核对的 open/read/close，只读有界文本 |

`tools/probe_network_abi.py` 执行原始 AP 的 clock dispatcher/conversion、system wrapper 与 attr setters；硬件 tick leaf、scheduler/task_spawn 和 waitpid 是显式主机模型。它验证 0/1/9999/10000、大于 32 位 ticks 的秒/纳秒转换，缓冲哨兵、无效 clock id、NULL、system argv/属性和 wait-status 返回值。**这是静态/主机证据，尚未真机验证。**

`network_monotonic_ms()` 读取真实单调时钟，按 lwIP 的 `u32` 自然回绕语义转换毫秒；临时时钟错误保留上一有效值，不将网络时间拉回零。网页 uptime 使用独立的秒值读数，避免在约 49.7 天毫秒计数回绕后显示错误。

重启请求不在 PAN worker 上执行阻塞 system：`chaos_network_reboot()` 通过已核对的 `nsh_spawn` 创建独立 `chaos.web.reboot` task，只允许固定 `reboot` 字符串。PAN 后续时钟 tick 用 `WNOHANG=2` 回收该任务；没有正的匹配 waitpid 结果前，重复请求返回 busy，不创建第二个任务。生产封装不使用被 target pack 标记 FORBIDDEN 的 pthread ABI，也不接收 HTTP 提供的任意 system 命令。

meminfo 最多读 4096 字节/32 次 read，EINTR 有预算、EAGAIN 不自旋；HTTP 当前提供 511 字节缓冲并补 NUL。读取失败时不设置 HAVE_MEMINFO；没有恢复/验证的 heap 数值保持不可用，不用假数据代替。

## 网页 NSH 生命周期

PAN worker 是导出 API 的唯一调用者，也是 TX 的唯一生产者、RX 的唯一消费者。NSH worker 使用已核对的 p67 task_spawn、PTY ioctl 和 console ABI，详见 [NSH-SPAWN.md](NSH-SPAWN.md) 与 [NSH-PTY.md](NSH-PTY.md)。复用的是 SPSC 类型代码，不是本地终端的静态队列实例。

发送要么整块入队要么失败；短写、EINTR、EAGAIN 和 LF 原样保留。RX 固定 8192 字节，溢出继续泵送并显示准确丢弃字节数，不因浏览器未读取而堵住 shell。

停止是异步的：先将状态置为 Stopping，禁止后续发送/重开，由 worker 丢弃尚未发送的 TX suffix，再发送 `Ctrl-C + 256 个退格 + exit\n`。唯一 LF 位于 exit 末尾，避免先执行未完成的 shell 命令；p67 readline 的最大命令行是 255 字节，256 个 BS 足以清空已有文本。

仅当 waitpid 返回准确 child pid 后，才排空有界尾输出、关闭 master、释放未委托给 stock exit callback 的遗留 console，最后发布 Exited/Failed。done 标记、EIO/EPIPE、等待错误都不能替代正 waitpid。prepare/spawn 期间到达的停止不会被 Running 状态覆盖；停止后没有回收证明时不允许创建新会话。

若命令忽略 SIGINT 或长期不返回，状态可能保持 Stopping；本实现不会以危险强制释放换取表面上的“已停止”。常驻 task 与模块同寿命，启动网络/终端后不支持热卸载模块。

## 认证与密码变更

账号固定 `chaos`；手表键盘允许 8–31 个 printable ASCII 字节密码。默认没有密码，胶水层拒绝启用 HTTP。setter 校验全部通过后，先通过 `chaos_httpd_enable(0)` 请求旧认证失效，再在短临界区替换同步快照；不自动重新开启服务。监听端口、HTTP client 和 Web NSH 的回收始终在 PAN worker 处理，UI 不跨线程直接释放它们。无效密码不改变旧密码或服务状态；同密码的幂等写入不会无故停止服务。

## 可重复验证

```sh
python3 tools/test_web_nsh.py
python3 tools/test_network_platform.py
.venv/bin/python tools/probe_network_abi.py
```

本次结果：Web NSH 25 项 Rust 主机测试通过；platform 7 项 Rust 主机测试通过；net_glue 的 ASan/UBSan 测试通过（校验/失效顺序、1–512 字节快照缓冲、状态有效位）；精确 AP clock/system 指令探针通过。未连接设备、未执行任何真实重启；蓝牙、VFS、信号时序、内存压力和长期运行仍需真机验收。
