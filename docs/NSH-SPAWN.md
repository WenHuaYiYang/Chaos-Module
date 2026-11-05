# p67 / 3.101.043：NSH spawn、file-actions 和退出 ABI

本页记录从 **小米手环 10 Pro / p67 / Vela 3.101.043** 原始 AP 独立恢复的 ABI。不是 Band 11/q66 的地址平移，不改 Canopus target pack 的审核状态，不使用被标记为 FORBIDDEN 的 pthread 候选。

- AP：`research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin`
- SHA-256：`519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec`
- 指令 VA：`0x0C0C0000 + 文件偏移`；常量 data alias：`0x2C0C0000 + 文件偏移`。
- 下表为可调用的 **Thumb 奇数地址**；文中反汇编调用点使用偶数地址。

## 可调用入口

| 入口 | Thumb 地址 | 形态与用途 |
| --- | --- | --- |
| task_spawn | `0x0C209771` | `(name, entry, actions*, attr*, argv, envp) -> pid 或负 errno`，六参 |
| posix_spawnattr_init | `0x0C1EA725` | `(attr*) -> 0 或正 errno`；20 字节、4 字节对齐 |
| posix_spawnattr_setschedparam | `0x0C1EA7A1` | `(attr*, uint32_t *priority)`，第二参不是整数本身 |
| posix_spawnattr_setstacksize | `0x0C1EA7C5` | `(attr*, uint32_t bytes)` |
| posix_spawnattr_setflags | `0x0C1EA781` | `(attr*, flags)`，写字节 `+0` |
| posix_spawnattr_destroy | `0x0C1EA7E1` | `(attr*)`，本固件只返回 0 |
| file_actions_init | `0x0C1EA711` | `(head**) -> 0`，将单个 32 位 head 清零 |
| file_actions_adddup2 | `0x0C1EA63D` | `(head**, oldfd, newfd) -> 0 或正 ENOMEM(12)` |
| file_actions_destroy | `0x0C1EA6E5` | `(head**) -> 0`，遍历并配对释放所有节点，然后清零 head |
| user zalloc | `0x0C1F90BD` | `(size) -> pointer/NULL`；只供解释节点/console 所属堆 |
| user free | `0x0C1F8FF9` | `(pointer)`；与上面的分配器配对 |
| waitpid | `0x0C2063C5` | `(pid, int *status, options) -> pid / 0 / -1+errno` |
| usleep | `0x0C1F5D0D` | `(uint32_t microseconds)`，`20000` 转为 20 ms |
| errno location | `0x0C1E45BD` | `() -> int*`，当前 TLS 存在时返回 TLS+0x3C |
| NSH console constructor | `0x0C238E29` | `(mode)`，返回新分配的 0x5B8 字节 console |
| nsh_session | `0x0C23DDAD` | `(console, print_banner, argc, argv)`；argc<=1 进入读取循环 |
| native console exit | `0x0C238C19` | `(console, result)`，释放自有资源后终止当前任务，不正常返回 |

`system()` 的 stock caller 在 `0x0C2403BC/3CE/3DC` 依次调用 priority/stack/flags setter；`0x0C2403FE` 调 task_spawn。模块可以使用已核对的创建链启动自己的驻留 worker task 和 NSH child task，无需未经核对的 pthread_create。驻留 worker 的生命周期仍是模块生命周期；本页不证明模块热卸载安全。

## spawn attributes：不能把 flags 写成 stackaddr

20 字节属性结构为：

| 偏移 | 大小 | 含义 |
| --- | --- | --- |
| +0x00 | 1 | flags；stock 使用 4，表示应用指定优先级 |
| +0x01 | 1 | priority；stock 使用 100 |
| +0x02 | 1 | scheduler policy；由 init 读取当前策略 |
| +0x03 | 1 | padding，调用方先零初始化 |
| +0x04 | 8 | signal mask；init 清零 |
| +0x0C | 4 | stack size；init=0x800，stock system=0x2000 |
| +0x10 | 4 | **stack address**；init=0，必须保持 NULL 才由系统分配 |

`0x0C209788` 读取 attr+0x0C；`0x0C20978A` 读取 priority；`0x0C209792` 读取 attr+0x10 并作为 initializer 的 stack address。把 4 写在 +0x10 会把地址 4 当成栈，不是 flags，存在立即崩溃风险。worker/shell 可以经已核对的 stack setter 请求 32768 字节；probe 已执行并验证该值及所有边界哨兵。

task_spawn 在入口读取 `*actions` 得到链头，传给 initializer；不是把 head 的地址当第一节点。第五、六参位于调用栈，分别是 argv/envp。envp=NULL 时调用 `0x0C202364` 取得当前环境。创建流程中的 initializer、属性应用和激活在 task_spawn 返回之前完成，因此局部 attr/actions 容器可在返回后销毁，但这不等于可提前销毁子任务实际使用的应用数据。

## file-actions 和父/子文件表隔离

固件 adddup2 分配 16 字节节点，实际布局：`next@+0 (u32)`、`type@+4 (u8)`、`oldfd@+8 (i32)`、`newfd@+0xC (i32)`。type=2 表示 dup2；append helper `0x0C1EA618` 保持提交顺序。init/destroy 的容器仅一个 32 位链头。不要自己按其它机型的布局释放节点。

分配器 `0x0C1F90BC` 与 free `0x0C1F8FF8` 从 **同一个 `0x200F4C28` heap pointer slot** 读取堆，分别调用 mm_zalloc / mm_free。探针执行了这两个实际 wrapper，在底层堆模型断言配对；同时覆盖分配失败时 adddup2 返回正 12 且不破坏原链表。生产应调用 file_actions_destroy，不需要直接访问该堆槽，也不能用 kernel free。

隔离链：

1. task_spawn 分配 0x4E8 字节 TCB，并写 TCB+0x40=0x8000（低两位为正常 task，而非共享组 pthread）。
2. initializer `0x0C208C8C` 调 `0x0C202824`；正常 task 的 group 指针置为 `TCB+0xE8`。
3. initializer 在 `0x0C208CCA` 调 `0x0C202A48(child_tcb, action_head, 1)`。
4. file setup 将当前父 group 与 child group 比较。不同则调用 `0x0C1B932C(parent_group+0x2D0, child_group+0x2D0, actions, 1)`，克隆文件表，再调用 actions dispatcher。
5. dispatcher `0x0C209544` 始终把传入的 **child TCB** 作为第一个参数；type2 调 `0x0C1B97EC(child, oldfd, newfd)`。该函数的文件表 getter `0x0C2055D4(child)` 返回 `[child+8]+0x2D0`。

PTY 桥可以把 slave 映射到 child 的 0/1/2，并再用 slave 覆盖 child 继承的 master fd，以免 child 持有自己的 master 端。父 worker 的文件表不执行 dup2/chdir。探针执行实际 group selection、file setup、dispatcher 与 getter；仅低层文件表克隆/dup2 的 VFS 叶子和锁为模型，断言所有写均指向 child。PTY ioctl 的独立证据见 `NSH-PTY.md`。

## argv：交互式会话必须避免重复传任务名

任务参数 builder `0x0C208FEC` **自动把 name 放入 argv[0]**，随后才复制调用方的 argv 项，最终放 NULL。

- `task_spawn(name, entry, actions, attr, NULL, NULL)`：entry 得到 argc=1、argv[0]=name。
- 传入 `argv=[name,NULL]`：entry 得到 argc=2、argv[1]=name。

后一种情况下 nsh_session 会把第二个参数当脚本路径，并不会进入交互循环。probe 执行实际参数 builder、实际 task stack-frame allocator 与实际 strlcpy，验证两种布局。任务入口是 `int entry(int argc, char **argv)`；进入 nsh_session 前不额外补 argv[0]。

## console 生命周期与退出

constructor 初始化：`mode(u8)@+0x4A8`、stdin=0@+0x4AC、stdout=1@+0x4B0、stderr=2@+0x4B4；命令行缓冲从 +0x4B8 开始、0x100 字节。新 console 的 exit callback 位于 **该实例 +0x2C**，指向 `0x0C238C19`，不是全局 vtable。

stock caller `0x0C2390AC`：constructor → session → `[console+0x2C](console,rc)`。session 第二参非零仅先打印欢迎文本；真正是否进入循环由 argc/argv 决定。`exit` 内建可直接调用实例 callback，因此仅在 session 返回后设置 done 不够。

具体所有权证据：

- `nsh_session` 的读取失败分支 `0x0C23DE48..0x0C23DE6A` 输出错误后返回 0，**不释放 console**。probe 执行这段真实 session（仅 prompt/input/output 为模型），验证原分配仍存活。stock caller 在 `0x0C2390CA` 读取实例 +0x2C，`0x0C2390D0` 委托退出。
- 内建命令表 `0x2CCDB428` 将 `exit` 指向 `0x0C239131`；其 `0x0C239132` 设置 result=0，`0x0C239134` 读取 `[console+0x2C]`，`0x0C239138` 直接调用。probe 执行该内建及原生 callback，观察到释放与 task exit(0)，不会回到 session 循环。
- 实例 +0x08 是独立 release callback `0x0C238D7D`：关闭非标准 fd、在 `0x0C238DB0` 释放 console，然后返回（不退出任务）。已定位的 NSH 内实际调用 `0x0C2377AA..7AC` 属于 stock 初始化/启动脚本流程，其 console 来自 `0x0C23778A` 的独立 constructor；并非 `nsh_session` 释放主会话。session 本体与 parser `0x0C237E84..0x0C238A84` 中没有直接释放 console 或调用 +8 的路径。
- 实例 +0x00 的 clone callback `0x0C238CF5` 新分配 0x5B8 字节并重装原生 callback，仅从来源读取 mode，不复制 Chaos 替换的实例 exit callback。probe 验证 clone/release 不释放原 console。重定向恢复 callback 实际位于 **+0x28**（`0x0C238DC5`），只关临时 fd 并恢复 fd 三元组、不 free；**+0x18 是错误输出**（`0x0C238BB9`），不可误认成析构入口。

native exit 会关闭非标准 stdout/stderr/stdin，避免相同 stdout/stderr 双关，随后在 `0x0C238C4C..4E` 配对 user_free(console)，最后调用 task exit `0x0C1EBD08(result)`，不返回。probe 执行该实际 callback，验证 close 顺序、同 fd 去重、同堆 free 和 exit 参数。模块只可替换自己新建实例的 callback：**先原子清空共享的自有 console 指针**，再发布结束标记、委托保存的原 callback。否则 worker 回收 child 后会拿到已释放的悬空指针，造成重复释放。不要修改全局表，也不能从父 worker 调用 console destructor（其中的 fd 属于 child）。

收到信号的 child 可能不经过实例 callback。worker 必须以已核对的 nonblocking waitpid 检测退出；**成功回收 child 后**，若仍有自有 console 指针，可只用配对 user_free 回收其内存，不要调用会触碰父 fd 的 destructor。不得在 UI 线程调用阻塞 waitpid。

异步终止若恰好落在「清空共享指针 → native free」之间，仍可能泄漏单个 console；这是优先避免 double-free 的所有权边界，不能宣称任意异步信号下严格 exactly-once。这里不证明所有第三方 NSH 扩展命令都遵守相同所有权，也不证明热卸载安全。

## 与用户提供的 Canopus 10 Pro target pack 交叉核对

核对目录：`/Users/ikun_cxkpro/Projects/MIBAND10PRO-AUTORUN/modules/Canopus-Module/targets/xiaomi-band-10-pro-3.101.043`。`target.toml` 固件 SHA-256 与本页完全相同，标注 Cortex-M33、ARMv8-M Main、Thumb2、小端、soft-float、best1503-v1。未修改任何 target record 或 approval 状态。

`symbols/*.nuttx.{open,read,write,ioctl,errno_location}.json` 的 callable 分别为 `0x0C1D0A29 / 0x0C1D129D / 0x0C1D2641 / 0x0C1D01A5 / 0x0C1E45BD`，与独立恢复一致；`EVID-IO-1043-001` 也给出 stock console open(flags=3)、public open 的变参序言以及读写错误转 errno。调用 open/ioctl 时仍须按具体 flags/request 提供其所需第三参，不能把两个固定参数的生成 wrapper 泛化到所有请求。

pack 的 close record 是 `0x0C1B9D81`，而 NSH 原生 callback 使用 `0x0C1B9E49`。两者实际代码不同：前者在 `0x0C1B9E00` 返回负 EBADF(-9)，后者在 `0x0C1B9ECA..ED6` 写 errno=9 并返回 -1。Chaos 公共错误处理应沿用已核对的后者，不能仅因符号名相同替换地址。pack 的 `heap_malloc=0x0C1F903D` 也不替代 console/file-actions 自己使用的 zalloc/free 配对。

`pthread_create_internal` record 明确为 **FORBIDDEN / REJECTED**，候选 `0x0C204A79` 不可调用。当前 canonical symbols 中没有 task_spawn、spawnattr/file-actions、waitpid、nsh_session/console records；本页与 probe 提供的是 Chaos 独立的静态/host 恢复证据，**不是**对缺失记录的补充批准。pack 本身也标记相关公开 IO device=`not_probed`。

## waitpid：WNOHANG 是 2，不是 Linux 的 1

公开 wrapper `0x0C2063C4` 调内部 `0x0C206184`，将负 errno 转为 -1 并通过当前 errno pointer 写正 errno。

内部 `0x0C206284/0x0C20622C` 用 `tst options,#2` 判断非阻塞模式；child record flags+4 的 bit2 表示退出。probe 执行实际公开/内部 waitpid、signal-set 构造、child record 查找/摘链/回收以及 errno wrapper，模拟的只有任务查找和 TLS：

- `waitpid(owned_pid,&status,2)` 且 child 活着：返回 0，不改 status、不回收 record，不走 signal wait。
- child 已退出：返回 pid，status=`record.exit_code << 8`，实际摘链并将 record 归还 `0x200F5CB0+0x100` 的内部空闲表。
- 已回收或没有该 child：返回 -1，errno=10 (`ECHILD`)。
- BASEPRI 在所有已测试返回路径恢复。

`options & 0x20` 是内部不回收分支，但 Chaos 无需使用。probe 针对 normal task 的保留 child-status 路径（parent group flags bit0=0）建立测试；它不等价于任意全局 signal policy 或完整真实调度仿真。

## 可重复执行与验证等级

运行：`python3 tools/probe_nsh_spawn.py`（依赖 unicorn；缺依赖或镜像显式退出 77）。本次使用 `/tmp/chaos_capstone/bin/python tools/probe_nsh_spawn.py`，六组全部通过。脚本先锁定完整 AP SHA-256，再将原始机器码映射到 Unicorn 执行。

实际执行：属性 init/setters、file-actions init/adddup2/destroy 和对应堆 wrapper、任务组选择、文件 setup/dispatcher、task_spawn 参数转发与属性应用、task argv 构造、waitpid 判断与记录回收、console 构造/clone/release、session EOF 返回、内建 exit/console 退出、usleep 换算、errno。

模型边界：底层 heap 分配释放、内核调度/激活、VFS 叶子和锁、TLS、session prompt/输入/输出、nanosleep、最终 task exit。不运行未知地址，不覆盖硬件寄存器，不调用 FORBIDDEN pthread。session probe 只覆盖 EOF 所有权，不等价于执行完整命令 parser。**这是静态恢复 + host instruction execution 证据，尚无真机验证**；触摸、实际调度、PTY信号、GPU/cache、功耗和长期崩溃稳定性仍需设备测试。本页不提升任何 target-pack APPROVED 状态。
