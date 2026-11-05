# p67 / 3.101.043 PTY 与交互式 NSH 控制 ABI

本文只适用于小米手环 10 Pro（p67）固件 3.101.043，是对 `NSH-ABI.md` 的 PTY 专项补充。结论来自本仓库的原始 AP 镜像及该镜像机器码的主机模拟执行，不使用 Band 11 / q66 的函数地址，也不改变 target pack 中任何 candidate 的批准状态。

## 核对对象与复现

镜像：`research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin`。

SHA-256：`519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec`。

代码地址 = 文件偏移 + `0x0C0C0000`，只读数据别名 = 文件偏移 + `0x2C0C0000`。下文反汇编地址为偶数；作为 Thumb 函数指针调用时必须设置 bit 0。

无第三方依赖的静态签名、fops、ioctl 跳转表检查：

```sh
python3 tools/probe_nsh_pty.py --static-only
```

执行固件机器码的主机测试（本仓库 `.venv` 中已具备 Unicorn）：

```sh
.venv/bin/python tools/probe_nsh_pty.py
```

测试不是重新用 Python 实现 PTY 算法，而是映射经过 SHA-256 核对的原始镜像，实际执行 PTY ioctl/write、VFS file_ioctl、getpid 和 NSH readline/iscntrl 的 Thumb 指令。互斥锁、信号量、底层管道、kill 以及 readline 的字符 I/O 回调在测试中建模。没有执行调度器、真实设备驱动或物理硬件。

## 实际设备操作表

PTY fops 数据地址为 `0x2CCBEA50`，PTMX fops 为 `0x2CCBEA80`。`0x0C1ACA34` 与 `0x0C1ACA5A` 的设备注册路径引用前者。

| 操作 | 表内偏移 | Thumb 函数指针 |
|---|---:|---:|
| PTY open | 0x00 | 0x0C1AC61D |
| PTY close | 0x04 | 0x0C1AC569 |
| PTY read | 0x08 | 0x0C1AC769 |
| PTY write | 0x0C | 0x0C1AC485 |
| PTY ioctl | 0x14 | 0x0C1AC2E1 |
| PTY poll | 0x20 | 0x0C1AC1F9 |
| PTY unlink | 0x2C | 0x0C1AC711 |
| PTMX open | 0x00 | 0x0C1ACADD |

`0x0C1AC9C0` 构造器分配 `0x108` 字节的共享对象。master endpoint 位于对象 `+0`，slave endpoint 位于 `+0x70`，两者的首个字指向共享对象。endpoint `+0x48` 是 master 标志，`+0x4C` 是前台 PID，初始值为 `-1`。共享对象 `+0xE1` 是锁定标志，`+0xE3` 是单字节 PTY 编号，`+0xE8` 是锁定信号量，`+0xF4` 是互斥锁。生产代码不应直接访问这些私有字段；它们仅用于证明 ioctl/信号语义及构建验证夹具。

## ioctl：不能照搬另一机型的参数形态

驱动入口 `0x0C1AC2E0` 从 `file +0x10 → inode +0x18 → endpoint[0]` 找到共享对象。普通终端请求通过基址为 `0x0C1AC32C` 的 TBB 跳转表分派，索引为 `request - 0x101`。

| 请求 | 043 中的值 | 第三个参数 | 已确认行为 |
|---|---:|---|---|
| TCGETS | 0x101 | termios 缓冲区指针 | 复制 endpoint +0x50、+0x58、+0x54 至缓冲区 +0、+4、+12 |
| TCSETS | 0x102 | termios 缓冲区指针 | 反向写入上述三个字段 |
| 认领前台 PID | 0x118 | **PID 整数值** | PID 非负且旧槽位为负时成功，否则返回 -22 |
| 释放前台 PID | 0x119 | 忽略 | 将 endpoint +0x4C 设为 -1 |
| TIOCGPTN | 0x12B | **32 位 int 指针** | 从单字节 PTY 编号扩展后写满 4 字节；空指针返回 -22 |
| TIOCSPTLCK | 0x12C | **整数值**，0 解锁，非 0 上锁 | 解锁会 post 信号量；已解锁时再次解锁不会再次 post |
| TIOCGPTLCK | 0x12D | **32 位 int 指针** | 返回锁定标志；空指针返回 -22 |
| FIONREAD | 0x306 | int 指针 | 仅转发至本 endpoint 的读管道 |
| 输出管道查询 | 0x307 / 0x308 | 转发参数 | 仅转发至本 endpoint 的写管道 |
| FIONBIO | 0x30A | **32 位 int 指针** | 先处理本 endpoint 的读管道，再处理写管道 |

不支持的请求返回 `-25`（ENOTTY）。表中负返回值是驱动 / 内部 file_ioctl 层语义，不表示公开 libc ioctl 必然原样返回负 errno；公开包装器可能转换为 `-1` 并设置 errno。

### 非阻塞不是 0x403

PTMX 的 stock open 路径在 `0x0C1ACB7A` 使用 flags `0x403` 打开内部 `/dev/ptyN`。这不代表 NONBLOCK。实际 VFS `file_ioctl` 入口为 `0x0C1CFF88`，FIONBIO 分支在 `0x0C1D0086` 解引用 `int *arg`，设置或清除内部 file flags 的 **0x40** 位。底层驱动返回 0 或 -25 时，VFS 均能处理这一请求。

PTY 的首次 open 通过 `0x0C1A5F88` 构建 flags 为 `0x400` 的管道，初始为阻塞模式。需要避免 UI/worker 因读空管道卡住时，必须正确调用 FIONBIO，而不是假定 `open(..., 0x403)` 已经非阻塞。

每个 endpoint 有独立的内部 file：master 读/写分别为共享对象 `+8` / `+0x28`，slave 读/写分别为 `+0x78` / `+0x98`。对 master 调用 FIONBIO 会改变 master 两个方向，不会同时把 shell 所在 slave 端改成非阻塞。不要对 slave 使用同样设置，否则 stock readline 可能把暂时无输入当作 EOF。

## Ctrl-C 的 PID 所有权

实际 PTY write `0x0C1AC484` 在 master 写入时读取 slave 前台 PID（共享对象 `+0xBC`），slave 写入时读取 master 前台 PID（`+0x4C`）。输出处理标志最低位开启且 peer PID **大于 0** 时，字节 `0x03` 触发 `0x0C207A00(pid, 2)`，调用点为 `0x0C1AC546`。否则它只是普通管道数据。

因此，Band 11 参考实现中的 `ioctl(slave, 0x118, 0)` **不能复制**：这里的 0 不是“当前进程”，而是把前台 PID 槽位真的设为 0。随后 stock NSH 用真实 PID 再次认领会得到 EINVAL，Ctrl-C 也不会发出信号。

043 的 stock NSH 会自行维护前台 PID：

1. `0x0C23C4F0` 调用 getpid，然后经 console 的 `+0x14` 回调传入请求 `0x118` 和真实 PID。
2. 外部命令路径 `0x0C23E444` 对产生的子任务 PID 发出 `0x118`，之后 waitpid。
3. 命令结束路径 `0x0C23E4D0` 发出 `0x119` 释放槽位。

getpid 的 Thumb 地址为 `0x0C2093ED`：读取全局当前 TCB 槽 `0x200B24D4`，再读取 TCB `+8` 的 group 指针和 group 首字 PID；当前 TCB 为空时返回 0。建议使用交互式 stock NSH session，保留原 console ioctl 回调，让其拥有前台 PID 生命周期；终端 UI 仅向 master 写入 Ctrl-C，不预先占用 PID 槽。

## Ctrl-D 不等于 EOF

此固件 PTY write 不为 `0x04` 提供 EOT/关闭语义，它将该字节写入管道。NSH readline 入口 `0x0C240128` 只特判退格、DEL、ESC、LF、Tab；之后经实际 `iscntrl`（`0x0C1E4244`）过滤控制字符。因此 Ctrl-D 被丢弃，不会退出会话。

主机探针实际执行此路径：PTY 写入 `0x04` 产生一次普通管道写；readline 收到 `[0x04, LF]` 时输出仅为 `LF, NUL`。真正 EOF 来自底层 getc 回调返回 -1。界面应提供明确的退出操作 / `exit` 命令，不得把发送 Ctrl-D 宣称为可靠退出；也不要仅关闭界面就强行终止仍在执行命令的 shell。

## 验证覆盖及边界

探针包含 11 项机器码执行测试：PTY 编号 32 位写入、锁定/解锁和值参数、FIONBIO 双向转发及实际 VFS flags 修改、底层 ENOTTY 回退、FIONREAD 转发、termios 字段读写、无效请求、Ctrl-C 信号目标、PID=0 反例、Ctrl-D 过滤、当前任务 group PID。

这些证据足以排除直接照搬 Band 11 常量/参数造成的已知 ABI 错误，但不能证明线程调度、资源释放、信号交付、真实输入输出、长时间运行、退出竞态或固件升级兼容性。生产后端仍需有界队列、单会话、错误即停止、非 UI 线程调用以及真机回归。未获批准的 pthread candidate 不在本次验证范围，也没有因本次 PTY 验证而变为可用。
