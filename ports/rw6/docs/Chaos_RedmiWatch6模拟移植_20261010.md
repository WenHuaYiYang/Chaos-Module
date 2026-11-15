# Chaos 移植到 Redmi Watch 6：模拟线开工记录（2026-10-10）

> 目标（用户设定）：做完整移植到 Redmi Watch 6 的 Chaos 版本；无真机，**全靠模拟和逆向，每个锚点自行模拟验证**。
> 本文件是**模拟线**的第一轮记录：让 p65 固件在 flint 上跑起来 + 逐堵点推进。
> 评估与可行性证据在 `docs/Chaos_RedmiWatch6移植评估_20261010.md`。

## 1. flint p65 适配（本轮已落地的全部改动）

### 新增 profile（三件套）
- `flint-vm/profiles/best1502_r0.soc.json` — 从 best1503 复制，SRAM 0x160000→**0x1A0000**（RAM3 0x140000），
  icache_ctrl 加两个别名 0x23FFA000 / 0x03FFA000（p65 固件专用，见§3）。
- `flint-vm/profiles/watch6-p65.board.json` — display **432x514**（镜像自证）、pitch 1296、driver 先按 icma3312；
  touch/nand/charger/analog 先照抄 p67 **待核**（chip_id 按校验器规则留缺项）。
- `flint-vm/profiles/firmware-watch6-p65-100043.json` — boot/vector_table/relocate/init_calls/assertions 全自证：
  - boot.pc=0x0C0C0011、MSP=0x2019FFE0、MSPLIM=0x2019F3E0、CONTROL=0
  - vector_table：build_fn=**0x0C0C0090**、base=**0x20166000**、slots=**110**（043 是 94）、
    src_flash_table=0x2C0C2208（源表 [0]=0x2019FFE0 与 crt0 互证）、default_handler=0x0C0C0085（自环跳板）
  - relocate：copy_src=0x2CBFF308、dst=0x2019E724..0x2019E728（**只搬 4 字节，异常小，待核**）
  - init_calls=[0x0C0C0090, 0x0C0C01D4, 0x0C0C0394]、preinit=0x0C0C1B58
  - rtos_sites/lvgl_sites 暂 unmodeled

### 校验器/模拟器工具的通用化改动（对 043/o66 目标行为不变）
- `flint-vm/tools/flint_profile.py`：SoC 改按 board 声明的 `soc` 名现取（默认目标仍走 best1503_r0 原路径）；
  `AP_RAM_SIZE`/SDK 头（best1503_addr_map.h）对账改为**仅 best1503 SoC 生效**（p65 自证 0x1A0000 不误报）。
- `flint-vm/tests/unit/test_boot.c`：
  - [A]段槽数从硬编码 94 改为读 `vt.slots`（p67 行为等价）；
  - `--uart-stop` 从只盯 uart2 扩到 uart0/1/2（p65 的主日志在 **uart1**）；
  - 新增 **`--memfill <地址> <长度hex> <值hex> <第N条>`**：定点填内存区（验证"固件期望的初值"类假设）。

### 运行方式
```bash
python flint-vm/tools/flint_profile.py --board watch6-p65 --fw firmware-watch6-p65-100043 --out profile_p65.prf
./flint-vm/build/test_boot.exe flint-vm/build/profile_p65.prf --deep <条数> [--ramdump 20000000 1A0000 flint-vm/build/p65_sram.bin]
```
test_boot 默认三段的 [A][B] 已全过（18 项 0 失败）：向量表 110 槽建表、VTOR 写入、preinit 跳转全对上。

## 2. 启动链推进记录（每步＝已解决 + 证据）

1. **[解决] PMU 芯片 ID 检查**（crt0 早期，0x0C0C1E40 链）：
   固件读 ISPI cs3/page0/reg0，期望 `(v>>4)==0xC0D`；模拟器照抄 p67 的 preset 0x1530 → 死循环。
   **改 preset=0xC0D0（p65 固件自己的期望值 0xC0D<<4）** → 过。证据：改完立刻推进（固件行为自证）。
2. **[解决] ICache 控制器别名**：0x0C0C1F38 用 **0x23FFA000**（=0x07FFA000+0x1C000000）做 cache 解锁，
   镜像里 0x23FFA000/0x03FFA000 各 12 处、主址 0 处（p67 反过来）。**给 icache_ctrl 加别名** → 过。
3. **[解决] DEADBEEF 队列扫描**：0xC513884 扫 4KB（0x201724B0 起）找"非 0xDEADBEEF 字样"；
   模拟器 SRAM 初值 0（被 crt0 bss 清零）→ 判"队列非空" → 走"记录+动作"链。
   **--memfill …12M 填全 DEADBEEF** 后扫描返回 0（已验证 r0=0）→ 该检查可过。
   （注意：该检查过了之后**流程仍到关机**，说明下述堵点独立。）
4. **[当前堵点] "动作码 2" → 关机序列**：
   - 现象：主入口(0x0C974D74)打印横幅 `[ap]system power on ota test....`（仅此 36 字节日志）后，
     ~40 万条内走到 **sys_shutdown(0)**：SRAM `0x2016E804`（日志 ID 0x4041）→ PMU 掉电寄存器序列 → `b .`。
   - 已挖到的链：`0xC4C34D0 段`（算 1000×x/y 百分比 → 0xC4C6078(回调,0) 广播 → 0xC50EC4C(2) "动作 2"）
     → `0xC50EC4C` 里 `cmp r4,#2; beq` 直达"错误分支"（打印+shutdown）→ `0xC50EC4C` 由 0x2016EB82 包装
     （r0=0）→ `0x2016E804` shutdown。
   - 现场特征：**CONTROL=0（handler 模式）、异常 0 次、PRIMASK=1、BASEPRI=0x80**；p67 在同时刻已 CONTROL=2 + 异常 258。
   - 待答：**"动作 2"的枚举语义**（0..6）与"谁发起这个动作流程"；以及该链在 p67 的等价物（是否有）。
5. **主入口三个后续调用**（0xC962E88 / 0xC963080 / 0xC4C2E28）都是"秒回"（各 2-40 条），不是元凶。

## 3. 已建立的调试手法（本轮发明/验证）

- **追调用链**：`--site <入口> N` 拿 `lr`（调用者）与参数 → 逐层上爬；注意**入口常在 capstone 线性扫描的"假指令"之后**
  （池字被解成 `subs/adds rX,#imm` 是常见假象；真入口是 `push {..,lr}` 那条）。
- **RAMX 别名**：SRAM 代码执行时 PC 是 **0x0036xxxx**（0x2016xxxx 的别名，delta 0x1FE00000），挂 site 要用别名。
- **抓打印点**：先 `--uart-stop <子串>` 得"发送循环"PC，再挂"日志格式化函数"入口（0x0C9745B4→真入口 **0x0C9745B8**）拿 lr。
- **栈回溯**：卡死点的 sp + `--ramdump` 拿栈，读返回地址链（Thumb 返回地址是奇数）。
- **读缓冲生命周期**：`--wstop <addr> <len>` 抓写入者、`--rstop` 抓读取者（本轮用它证明"缓冲第一次写=crt0 清零、第一次读=扫描"）。

## 4. 主入口之后的完整执行地图（12.4M-13M 段，beat/site 实测）

```
0 - 12.4165M   crt0 init 链（0x0C0C0394 → 尾调用 0x0C0C0314 一族；含大量延时，12M 拍都在这）
12.416514M     主入口 0x0C974D74 被调用（lr=0x0C0C0063 = crt0 0x5E 的 bl），只调用这一次
12.420416M     横幅 "[ap]system power on ota test.... "（日志函数真入口 0x0C9745B8）
12.420686-726  主入口三个后续：0xC962E88 / 0xC963080 / 0xC4C2E28（全部秒回，2-40 条）
12.4207M-12.444M  主入口之后：0xC50BCxx 的"大区域数据填充"（每次循环 ~2k 条、反复数百次；
                填充步进 0x120 与 8 两种，写一个固定数据字铺满 ~0x3000+ 区域）
12.444175M     队列初始化 0xC514786：queue 结构 [+0x40]=长度0xC00、[+0x44/+0x48]=指针
                （指针 = [池] - 0xC00），随后 bl 0xC513798 用 **DEADBEEF** 填充队列
12.4450M       实际上在跑 0xC5137DC 的 DEADBEEF 填充循环（与上一条互证）
12.45M-12.86M  继续初始化（0xC4FF35C / 0xC4FFBD2 调用族等）
12.86M         ★堵点★ 0xC4C3330（4 参大函数，队列处理）→ 两条分支(r4=0 / r4!=0)在 0xC4C351C
                汇合 → 0xC4C355A movs r0,#2 → bl 0xC50EC4C → 见下
12.92M         sys_shutdown（0x2016E804，日志 ID 0x4041）→ PMU 掉电序列 → b . 死循环
13M+           卡在 SRAM 0x0036EB36（别名 0x2016EB36）
```

### 堵点细节（0xC50EC4C 动作分派，0xC4C3330 汇合点）
- `0xC50EC4C(act)` 的动作分派（0x0C50ECBC 起）：`act==2` 或 `act>6` → 0xC50ECF0
  （`bl 0xC0C1EF4` + `bl 0xC8BCBE0`）；其余码 → `BASEPRI=0x80; wfi` idle 循环。
  **注意：两条"队列空/非空"分支都汇合到 0xC4C351C，最终都调 0xC50EC4C(2)** ——
  即"动作 2 收尾"在这条链里是**固定行为**，不是"队列非空"的异常分支。
- `0xC4C3330` 的参数（现场）：r0/r2 = PSRAM 指针（0x3C2D7C70 / 0x3C2D6BB3）、r1 = 0x2D3；
  调用者 = 0xC4F74E8 包装（参数 0x1B2）→ 上游在 0x0C4F74xx。
- `0xC4F74B8/D2` 是**任务上下文保存/恢复**（stm+FPU s16-s31 + fpscr）——这一带是 RTOS 内核代码。
- 现场特征：**CONTROL=0、异常 0 次、PRIMASK=1、BASEPRI=0x80**；p67 同时刻 CONTROL=2 + 异常 258。
  PRIMASK=1 的来源未定（若为固件陷入某临界区未恢复，与"动作 2 收尾"可能是同一设计的两面）。
- DEADBEEF 队列实验结论（重要）：**队列空/非空不改变"动作 2"结局**（memfill 5M/12M/12.5M
  三个时机全试过，r0 从 0x1000 变 0 但流程不变）——**别再拿队列内容当抓手**。

## 5. ★堵点更正（重要）：不是"关机"——是 NuttX assert + board_reset

用"挂日志函数（0xC504A54，va_args 转发到 0xC504A2C）收集调用参数 + PSRAM dump 读格式串"
的手法让固件自己说话，结论推翻上一节的"软关机"表述：

- **12.4825M**：NuttX `_assert` 打印
  `"Assertion failed false: at file: mpu_setup:723 task: ... process: ..."`。
  参数逐项解出（site 现场 + 栈 varargs）：
  - cond = `"false"`（0x3C2D6BB3）
  - file/函数名 = **`"mpu_setup"`**（0x3C2D7C70）
  - line = **723**
  - process ptr = 0x0C4C2E29（带 Thumb 位的代码指针，疑似 task 入口）
- **fmt 池周边串**（0x3C2D7C40-0x3C2D7C9x）：`sram_text start %x size %x` /
  `sram start %x and len %d must be aligned` / `mpu_set_rbar_rlar` /
  `: addr=0x%08X len=0x%08X ap_attr=%d mem_attr=%d` —— **MPU/内存区配置模块**。
- **12.86M 的"PMU 掉电序列"**（0x2016E804，日志 0x4041）= **board_reset(mode 2)**：
  同一 fmt 池里有 `reset mode:%d`（传 2）、`Dump board_reset backtrace:`、`backtrace %2d:%s`、
  `R0:%08lx ... xPSR ... EXC_RETURN`（up_dump_register）、`PANIC!!! Bus Fault`、
  `task_startup.c`/`sem_init.c` 等串 —— 死循环是在**等软复位生效**。
- 断言调用点 0xC4C340C 在 0xC4C3330 大函数体内；该函数被多次调用
  （12.420M 主入口链 → 12.4626M "Current Version" → 12.4825M assert → 12.8M 队列处理）。

**嫌疑点（下一步主攻）**：**SRAM 区域布局/搬运参数与固件期望不符** ——
crt0 的 .data 搬运参数本身异常（copy_src=0x2CBFF308 归一化后越界、len 仅 4 字节），
而 SRAM 里 0x0036xxxx 的代码确实在 12.8M 后被执行（说明另有搬运机制在跑）；
`mpu_setup` 的"sram start/len must be aligned"断言正是会因区域非法触发的那类检查。
**注意 best1502 的 SRAM bank 划分是我们照推的（RAM3=0x140000），尚无固件自证**。

## 5.5 MPU_TYPE 修复（已落地，p65 的 RTOS 跑起来了）

断言点反汇编定案（0x0C966A8C 起，mpu_setup 本体）：

```asm
0C966A92  ldr r1,[pc,#0x100]        ; r1 = 0xE000ED00（SCB）
0C966A96  ldr.w r2,[r1,#0x90]       ; r2 = MPU_TYPE（0xE000ED90）
0C966A9A  ubfx r2,r2,#8,#8          ; r2 = bits[15:8] = MPU 区域数
0C966A9E  cmp r2,#8
0C966AA0  beq 0xC966AB8             ; ==8 才继续
0C966AB4  bl 0xC4F74E8              ; assert("mpu_setup", 723, "false")
```

模拟器读回 0 → 区域数 0 ≠ 8 → 断言。**修复**：`flint-vm/src/l2/irq/scb.c` 给
`0xE000ED90` 加行为，读回 **0x00000800**（区域数 8——固件 `cmp #8` 的期望即证据；
Cortex-M33 本就 8/16 区域可配，取 8）。p67 回归（默认 18 项 + 50M 深跑）不受影响。

**效果（1G 拍）**：
- 从"assert→board_reset 死循环"变为 **RTOS 活跃运行**：异常进入 1,992,880 / 返回 1,992,879、
  SysTick tick 8,739,015 次（load=0xF 已编程）、挂起稳定 257、TE 沿 19 次
- 驻留循环是正常的事件/服务处理（0xC51456E / 0xC50DB39 / 0xC4C65E2 轮转），
  高频点在 0xC514CFC/D06/D40 = **反复读写 SysTick CTRL/LOAD/VAL**（时基忙碌，正常）
- **但 lcdc 帧仍 0**——显示链还没到，继续加长跑（4G 后台进行中）并追踪显示初始化路径

### 启动日志里程碑（1G 拍，uart1 共 2860 字节，固件自述）
```
CHIP=best1502p            KERNEL=NUTTX          FLASH_DUAL_CHIP=1
BUILD_DATE=Sep 12 2026 07:22:16                REV_INFO=d516af0:nx_best1502p_ap
METAL_ID: 0
307/I/NONE  /  0 | bes_chip_init_early done:1
[ap]RPTUN_INIT_DECLARE 616 0xc950a8b / 0xc950a8f     ← 核间通信(RPMsg)节点注册
（其后是一大段以 "01 xx fc ..." 起首的十六进制 dump —— 外设初始化序列表）
```
对照 p67 手册的里程碑表：p65 目前到 `bes_chip_init_early`；**下一步等 `bes_chip_init_later` /
`screen_turn_on` / `lcdc 帧>0`**。

### uart1 增长时间线（--beat 加口计数后，100M 拍；卡点收窄）
```
10M:0   20M:36(横幅)   40M:552(版本信息段)   80M:1913   90M:2850   100M:2850(停)
```
- 日志停摆点 = **`RPTUN_INIT_DECLARE` 之后**（其后还有一大段 "01 xx fc ..." 十六进制表，
  打完就没再有新日志）。
- **该段是 p65 特有**：p67 的启动日志里 `RPTUN` 与 "01 xx fc" 形态均为 **0 处**（已 grep 对账）。
- 结论：p65 在 `bes_chip_init_early` 与 `bes_chip_init_later` 之间、**RPTUN 节点注册之后**
  的某段初始化里卡住（十六进制表疑为某初始化序列的 dump）。下一步追这段的"等待点"。

## 5.6 i2c0/i2c1/i2c4/i2c7 补齐（已落地，自旋从 1147 万次降到 17 次）

**根因**：p65 的 hal_i2c 实例表共 **8 项**（file 0x312CA8 起、每项 16 字节，表前后是
`hal_i2c_sm_*` 名字），模拟器只建了 4 个；固件先操作**实例 0（0x40005000）**，
`0xC970786` 等它的 `+0x70`（IC_STATUS）判据 `(值&5)==4`（TFE 置起、BUSY 清零）——
兜底读 0 ⇒ **11,469,999 次自旋**（且每轮调 0xC962E88 那个空 stub"让出"，
所以 log 全静默、RTOS 却看着活跃）。**与 o66 时代建 i2c2/i2c6 的实测形态逐字相同**
（同判据、不同实例号，soc 里那两条 _note 就是当年的记录）。

**修复（三处）**：
- `best1502_r0.soc.json`：regions + devices 各补 `i2c0(0x40005000)/i2c1(0x40006000)/
  i2c4(0x40062000)/i2c7(0x40065000)`，都 `inherits: i2c5`（同套 DesignWare 表不重抄）；
- `flint_profile.py` 的 i2c 名单从硬编码四名改为**动态枚举 `soc.devices` 里的 `i2c*`**
  （043/o66 的 soc 只有 4 个 ⇒ 行为不变，回归已验：默认 prf 538 行/48 区间）；
- `test_boot.c` 的 `il[]` 扩到 8 名、`g_i2c[6]→[8]`（数组大小第一次漏改导致 segfault，
  已修再跑）。

**效果**：`40005070 [i2c0 +0x70]` 读 **11,469,999 → 17** 次；i2c0 各寄存器
（+0004/+0034/+006C/…）出现正常读写序列——固件在跑 i2c 控制器初始化。p67 回归
（[A][B] 18 项 + beat）不受影响。

## 6. 下一步（按优先级）

**★判据修正（第 9 轮，重要）**：**"UART1 停在 2860 字节"不等于固件卡住**——抓"日志函数
0xC504A54"的完整调用时间线后发现**日志链一直在推进**（info 级不上串口而已）。实测里程碑：

```
early done:1 (~40M) → btdrv_start_bt_end (83.45M, fmt 0x3C2D616B 区)
→ "I2C info open id %d,..." x4 (83.9-84.1M, id=0/1/2/4 ← i2c0/1/2/4 打开)
→ [341M-493M] sensor/BT 周期日志（0xC4F1C53 每 ~50M 一次心跳; r5=0x386BF203 连号计数）
```

**判"推进"改用**：`--site 0C504A54 <大预算>` 抓日志调用时间线（比 UART 输出真实得多）。
**当前待答**：显示链（`board_lcd_initialize`）在 p65 的顺序里排在 btdrv/i2c/sensor 之后的何处；
继续抓 2G+ 的日志 fmt（参照 p67 顺序 lcd 在 i2c 之前，p65 显然不同，不能照抄）。
**方法工具**：全部走 `band-fw-reverse` 技能（Ghidra `text/fn/xref` + 影子源法 + site/wstop/ramdump）。

### 第 9 轮：日志链的内涵（PSRAM dump 读出的 fmt 词条）

| fmt 出处 | 内容（词条） | 含义 |
|---|---|---|
| 0x3C2D616B | `btdrv_start_bt_end` / I2C error 簇 | BT 驱动启动收尾（83.45M） |
| 0x3C2D6222 | `I2C info "open id %d, mode %d, dma %d, sync %d, speed %d"` | i2c 打开（84M, id=0/1/2/4） |
| 0x3C17C9C4 族 | `[AW86225] board_aw86225_initialize / Wrong Device ID / Check chip id failed / force to enter standby` | **触觉马达驱动**（每 ~50M 拍重试日志） |
| 0x3C17F0DC 族 | `[BAT] pmu_charger pin change / high_temp_end_charger--display-- / wait pmu_charger ready!!!` / `[batterry_log]:vbus=4656` | 电池/充电状态机（含"等充电器"步，vbus=4656mV 已检测到） |
| 0x3C2B503C 族 | `show logo / show hyperOS / show charging low power / show low power` | **显示链词条**（开机 logo/动画的触发词） |
| 0x3C2B4C48 族 | `board_product_audio_lowerhalf_init / subsys_loader_load_segment_image_from_nor / board_power_manager / bootup` | 音频/子系统装载/电源管理名字池 |

**判读**：固件此刻在"外设驱动初始化/轮询"的深水区（马达/电池/传感器都在动），
显示链（show logo 等）在其后；`wait pmu_charger ready` 是电池状态机的一步（带超时计算，
非死等——Ghidra `fn 0x437160` 反编译为时基+100/8 的比较逻辑）。继续跑 2G+ 等 `show logo`
或 `board_lcd_initialize` 的日志出现。

1. **稳态循环里"任务在等什么"**：驻留 PC 在 0xC514xxx / 0xC4C6xxx / 0xC4D0xxx / 0x394xxx
   多函数轮转（像正常调度 + 某周期任务）；SRAM `0x201949E0` 大函数读全局标志分流。
   办法：beat 加密 + site 对"等待循环"逐点定位。
2. **0x40082000 一族"门铃"寄存器（已结案，正常驱动）**：0x0C97467C/640/688 族写的是
   **0x1ACCE551**（ARM 调试/测试解锁魔术字）等值、**周期 2000 拍**、读回不判；
   p67 也写它（低频）。**不是卡点**。调用者在 0xC512C60 区（78M 后每 2000 拍一次），
   本线头未合——若要继续，用 site 挂 0xC512C5C 往上拿调用链。
   （方法：本轮的"影子源法"与 Ghidra 反编译细节见 `.qoder/skills/band-fw-reverse/SKILL.md`，
   本文件不再复述工具用法。）
3. **中断 5**（ISPR bit5 被 pend，PC 0xC9630EA）：soc irqs 表里没有 5 号的名字，待查来源。
4. `bes_chip_flash_init` 的 FLASH_SIZE 对账错误簇在镜像里存在（"Wrong FLASH_SIZE…"），
   p65 日志里未出现——已过或未到，暂不处理。
5. **判据口径不变**：`bes_chip_init_later` / `board_lcd_initialize` / `lcdc 帧>0` 三格里程碑。

**第 8 轮新增：AP↔BT 邮箱（mcu_mbox）建模全部继承自 p67，地址待重证**
- prf 里 mbox 的 `peer_ready.addr=0x200FE2E2`、收包环表 `0x2CCDACC0`、`tx.head/tail.base`
  等**全部来自 p67 固件的逆向**（soc 里 _provenance 写得明明白白）；p65 的固件这些位置必然不同。
- p65 实测：**门铃 39 次、回包 1、投递 1**（p67：门铃 39、回包 3、投递 2）；p65 的 BT 驱动
  在 **SRAM 0x3915xx**（首 PC=003915CC），与 p67 的 flash 0xC2316xx 不同。
- 下一步：dump 反汇编 SRAM 0x2015xxxx/0x201915xx 区（p65 的 BT 驱动），从中重证
  "对端就绪格"与"收包环"的真实地址；先确认"固件是否真在等 mbox 包"再投入（收包中断仅 2 次
  是弱信号）。

## 7. 第 10 轮：显示链全景定案 —— DSI 读失败是首发堵点（2026-10-11）

本轮把 2.2G 拍的完整日志时间线解出来（工具 `build/p65_scan/timeline.py`：site 命中 +
PSRAM 字符串表），显示链从"未到"改判为"**到过且已走到降级路径**"。

### 7.1 实测时间线（关键段，拍=指令数）

| 拍 | 事件 | 证据 |
|---|---|---|
| 214M-300M | `dsi read err,ret_state = -4, reset lcd and dsi.` ×10（间隔 ~9.6M） | site lr=0C4DE545，r2=FFFFFFFC，r4 从 10 递减到 1 |
| 309.58M | `unknown lcd to init!` | lr=0C4E32E5 |
| 309.61M | `up_fbinitialize=0x%x,0x%x` | lr=0C512D1B |
| 309.66-310.07M | LCDC 初始化全序列：Initialize LCDC driver fbmem → Configuring the LCD controller → Configure lcd periphery → Configure global register → Initialize lcdc layer → Enabling the display → lcdc_initialize done | lr=0C9683xx 共 7 条（串见 file 0x3085D9..0x308685） |
| 311.85-314M | 充电器链：BESPOWER 成功 → SY6103X 失败 → SC7061 失败 → `not find charger ic` | lr=0C4F44xx/0C4F52xx/0C4DE1xx |
| 324-336M | ADC ×13 / 电池状态机 / `vbus=4656` | lr=0C4F6B59 等 |
| **341.60M** | **`show logo`（包装函数 0x0C950818，参数=1）** | site lr=0C95082D |
| 342M-1.6G | AW86225/SIH6164/TYHX3507/mot6005/W11XX/HX322X 全部初始化失败并重试 | 各 `[XXX]` 日志簇 |
| 1.94G | `denug log call %p` | lr=0C964BCF |

### 7.2 判读

- 显示链**到过**：LCDC 驱动完整初始化、`show logo` 已执行。
- **没帧的根因链**：面板识别阶段 `dsi read err -4`（10 次重试后放弃）→ 固件判
  **unknown lcd** → 降级初始化：一条面板命令都没发（flint `命令0`）→ TE 未开
  （`屏状态：TE 输出关`）→ 层配置其实写全了（+0xFC=1296 行距、+0x104=432x514、
  +0xF4=0x20020000 帧源、+0x264 format）但**从没写帧请求**。`show logo` 只到
  "逻辑触发"层，无面板可推 → 帧=0。
- **DSI 控制器地址 = 0x40230000**（驱动 PC 0x0C96D4xx-0x0C96D8xx 访问它）。
  p65 的 DSI 是独立块 —— p67/best1503 的命令走 LCDC+0x280，而 p65 的 LCDC 访问清单里
  **没有** +0x280/+0x230/+0x140。0x40230000 落在 flint 通用 mmio 透传区（读返回上次写入值）
  → 驱动的等待位永远不变 → -4 超时。**这就是首发堵点。**

### 7.3 下一步（按此顺序）

1. **Ghidra 读 DSI 驱动**。读码口径注意：**p65 工程 base=0（file offset 空间）**，
   查询直接用 file offset（0x0C96D4F4 → file 0x8AD4F4）。找"读面板 ID"的
   写命令→等就绪→取回复寄存器序列，按其给 0x40230000 建最小模型
   （命令→立即完成；0xDA/0xDB/0xDC 答面板 ID 字节）。
2. **面板 ID 取值**：p65 同时含 icna3312 与 rm690c0 驱动字符串，ID 答多少要用
   Ghidra 读驱动里的比较字面量（o66 先例：ID 字面量与成功/失败串相邻）。
3. **各失败外设的总线/地址重证**（全部照"固件行为为准"）：充电器固件在
   **i2c0 addr 7**（8 条命令全 NACK、末端地址 7；p67 的 i2c3 口径对 p65 不成立）；
   AW86225/SIH6164/TYHX3507/mot6005/W11XX/HX322X 逐条从失败日志的"最近从地址"重证。
4. **aon_timer1 的 IRQ**：p65 固件用 **IRQ5**（NVIC ISER0 bit5 / IPR[5] / ICPR0 bit5，
   583907 次重臂），flint 接的是 IRQ46（p67 口径）；0xC962xxx 驱动同时访问 0x4008402x。
   改 profile 前先用 Ghidra 读码（file 0x8A2xxx/0x8A3xxx）。
5. `+0x228 = 0x1A000000` 的含义（p65 LCDC 独有）→ Ghidra file 0x8A7xxx 读码定。

### 7.4 本轮新工具/资产（build/p65_scan/，已 ignore）

- `timeline.py` / `timeline.txt`：site 命中 → PSRAM 字符串 → 完整日志时间线（226 条）。
- `psram_refs.tsv`：全镜像 PSRAM 字面量引用表（106054 条）——找"谁引用某个 PSRAM
  字符串/数据"的直路（p65 大量引用是"基址+偏移"或内联池，单字面量搜会漏）。
- `uart1_full.txt`：UART1 完整输出（2936B）——到 `yaffs: 0 blocks to be sorted...` +
  `offline_log [9:100]` + `log_service [11:100]` 后静默（p67 同型，不是卡死）。
- PSRAM 装载映射（上轮破译、本轮复用）：**file = PSRAM 偏移 + 0x2FDC4**。
- 043 的 Ghidra 工程已建好（对照用）；p65 工程 TMode 修复后重分析（进行中）。

## 8. 第 11 轮：DSI 最小模型落地（0x40230000）

### 8.1 协议定案（全部读自固件自己的 DSI 驱动，file 偏移）

| 件 | 位置 | 事实 |
|---|---|---|
| DSI 控制器 | **0x40230000**（窗口内碰过 +0x00..+0x74） | p65 独有；SDK 头与参考实现都没有这个窗口 |
| PHY 初始化 | file 0x8AD3F0 / 0x8AD474 | 写 +0x0C/+0x48/+0x60/+0x64/+0x70 |
| 写引擎 | file 0x8ADA0C（flush 在 0x8ADBE8） | 往 +0x14 塞字（0x5E100/0x1E02/0x11E04），写 +0x10 后轮询其 bit16，15 拍上限 |
| 读引擎 | file 0x8AD7E0 | 见下 |
| "dsi read err" 调用点 | file 0x41E4DC（重试 ×10）× → "unknown lcd to init!" | 期望 `lcd_dsi_read()==3`（3 字节 DDB） |

读引擎（0x8AD7E0）的寄存器协议：
1. 写 +0x10：`(旧 & ~0xFF0000) | 0x80000`（bit19 = 读模式，驱动自己写的）；
2. 写 +0x18 两笔：`(大小<<8)|0x37` 再 `(命令<<8)|0x06`（命令 4 = 读 DDB，大小 3）；
3. 写 +0x14：`0x5E104`、`0x5E106`；
4. 轮询 +0x10：`(v & 0x1F00) != 0` 就取低 8 位一个字节；计数（含当前字节）在 [15:8]；
5. 取空后再读一次进入解析：`buffer[1] & 0x3F` 是 DT（0x1A/0x1C 走长包支路），
   长度域在 buffer[2..3]，**载荷从 buffer[5] 起**拷贝（file 0x8AD952）。
6. 解析要求"取空那一次"的 +0x10 仍带 bit19 —— 4 个错误返回值里 -4 就是
   "把非 0x1F00 位当成计数/数据"的错分支（旧跑 10 次 -4 的来路）。

### 8.2 面板识别表（file 0x423340 / 0x41E598）

读回三字节拼成小端字后与候选比：

| 字 | 字节 | 分支 |
|---|---|---|
| 0x00200135 / 0x00200145 | 35/45 01 20 | rm690c0 · VSN 屏（日志 `######vsn p01`，init file 0x4208A0） |
| 0x00200245 / 0x00200235 | 45/35 02 20 | rm690c0 · TM 屏（`######TM p01`，init file 0x4209D0） |
| 0x00300145 / 0x00300245 / 0x00300135 | 45/35 01/02 30 | icna3312（init file 0x41F37C） |
| 其它 | | `#####UNKNOWN LCD` → 降级（就是上一轮测到的路径） |

本板先取 **0x00200135（rm690c0·VSN）**：`display.ddb = "350120"`（物理屏供应商归属未核）。
枚举不到已知 ID 时驱动会**重读 10 次再放弃**，所以 DDB 必须答成表内值。

### 8.3 落地件

- `flint-vm/src/l2/dev/dsi.c` / `dsi.h`：影子寄存器 + 读命令触发 + 长包应答。
- SoC JSON：regions/devices 加 `dsi`（base/size/regs/read_cmd/read_flag/size_flag）；
  board JSON 加 `display.ddb`（校验器查 6 位十六进制）。
- `flint-vm/tests/unit/test_dsi.c`：49 项断言（DDB 读全过程 8 拍计数/字节/取空位保留），
  `--sabotage` 3 处反写对照成立。
- `test_boot.c` 装配 + `build.sh` 四处（p65 profile 生成进构建门、DEV、单测、对照组）。
- 单测实测：`通过 49 项，失败 0 项`；sabotage 退出码 1。

### 8.4 第 12 轮：DSI 模型首跑定案（360M 拍实测）

**判据全达成**（`build/p65_scan/deep_dsi_360m_v3.log`）：

```
214.37M  read g_lcd_module_id is 0x00200135   ← 面板 ID 读出（= 喂进去的 DDB）
214.43M  init lcd rm690c0!
214.47M  ######vsn p01                         ← 识别到 VSN 屏，进 rm690c0 初始化
214.56M  init rm690c0 lcd finish!              ← 面板初始化完成（旧跑在此前要烧 10 次 -4 重试）
214.6-215.0M  LCDC 初始化全序列（比旧跑提前约 95M 拍）
257.73M  show logo                             ← 旧跑 341.6M
```

- `dsi read err` 归零；降级路径（unknown lcd）不再出现。
- 充电器探测也从"i2c 写全失败"推进到"值不匹配"（见 §8.5），说明总线/端点层已经通了。
- 仍 `帧0`：首帧还差"谁触发帧提交"——与 p67 同型（UI/服务起来后才画）。
- **站点预算坑**：`--site` 的"打印上限"是**全局一个变量**，多处 `--site` 时**最后一个的次数对全部站点生效**（v2 跑里第二个 `--site 0C4E59A 20` 把第一个的 400 顶成 20，日志只截到 187M）。

### 8.5 第 12 轮：充电器端点已挪到 i2c0（实测两阶段）

- 挪之前：`[SY6103X] i2c_write failed: -1`（无 ACK）。
- 挪之后（i2c0.charger_addr=7，v3 跑）：写全部 ACK，失败模式变成
  `THIS IS NO SY6103` / `THIS IS NO SC7061`（读到寄存器值不匹配）。
- 下一步：按 SY6103X/SC7061 驱动自己的期望值核端点寄存器答法（读码位置
  file 0x4F4453/0x4F521B 族，期待值在同一函数里现查）。

### 8.6 第 13 轮：启动链阶段地图（DSI 修复后的两跑对比）

**新发现：yaffs/rc.sysinit 段是"后段"里程碑。** 对比旧跑（2.2G，UART 2936B）与
新跑（DSI 修复后，UART 2850B）：新跑头 2850B 与旧跑逐字节相同，但**尾部的
`yaffs: 0 blocks to be sorted...` + `offline_log [9:100]` + `log_service [11:100]`
三行只在旧跑出现** —— 到 1G 拍时新跑还没走到那里。结合旧跑 1.6G 还在设备重试：
**板级设备初始化（AW86225/mot6005/W11XX 等重试）占用 341M-1.6G，rc.sysinit 在其后。**

**p65 的 rc 脚本原文都在镜像里**（照 GCC-Embedded 的 preproc 文本）：

rc.sysinit（file 0x2FEC7C 起）：
```
set +e
mount -t procfs "/proc" ; mount -t tmpfs /tmp ; mount -t romfs /dev/resource /resource
mount -t yaffs -o autoformat /dev/nand_data /data
offline_log start -w &
rptun start /dev/rptun/sensor        ← p65 特有：两个 rptun 对端（sensor/audio）
rptun start /dev/rptun/audio
mount -t rpmsgfs -o cpu=sensor,fs=/proc /mnt/sensor
kvdb_init
kvdbd &
miwear_rtc_checker &
chargerd &
```
rcS（file 0x2FEA9C 起）：
```
set +e
uname -a > /dev/log
bluetoothd &
losetup -r /dev/loop0 /system/zoneinfo/zoneinfo.img
mount -t romfs /dev/loop0 /mnt/zoneinfo
set -e
if [ ! -d /data/nobusiness ]; then miwear_bluetooth & ; usleep 1 ; fi
nfc_stack_bridge & ; miconnect & ; sleep 2
if [ ! -d /data/nobusiness ]; then miwear & ; fi
```

**到 UI 的判据链（照 p67 M3 文档 §74/§92 的同型结构）：**
`rc.sysinit 走完（kvdbd [pid] 打印出现）→ rcS 应用清单打印（[name] [pid:prio] 族出现
bluetoothd/miwear）→ miwear(main) 起 → LCDC 帧请求 → 帧>0`。
**当前卡点候选**（按 rc.sysinit 顺序）：`rptun start /dev/rptun/sensor`（p65 特有，
对端核在模拟器里不存在）、`mount rpmsgfs`、`kvdb_init`。判据：kvdbd 的 `[pid:100]`
打印在 UART 里出不出现（1G 跑为止：不出）。

**稳态循环（1G 末态实测，最近 24 次调用的重复环）**：
```
0C4DA534/0C4DA55E → 0C50E5DC/0C50E6B8 → 0C50EA48 → 0C9620B8（aon_timer 区）
→ 0C9518C4→0C951988（回调层）→ 0C50DEC4/0C512C28 → 0C512C5E→0C97467C（0x40082000 门铃族）
```
多个任务在轮转（不是单点自旋，热点最高 0.8%）。0x4D0836 是高频中转。

### 8.7 第 14 轮：两个 CI 门修复 + 全量 build 转绿

1. **MPU_TYPE 字面量违规**（门 A 类：手写代码不许出现 5 位以上十六进制字面量）：
   §5.5 的 MPU_TYPE 修复直接写了 `0x00000800u`，而全量 `build.sh` 自那以后没跑过 ——
   这次接入 DSI 后首跑才现形。修法：区域数进 profile（`core.mpu_regions = 8`，两个 SoC
   都加，带证据注释），`scb.c` 读 `core.mpu_regions` 后 `<<8` 组装 MPU_TYPE。
2. **test_dsi 的 sabotage 退出约定写反**：本仓库约定是"**sabotage 模式必须退出 0**
   （反写断言确实失败 → 对照组成立）"，不是常规的 fail_n>0→1。已按
   `return sabotage ? (fail_n > 0 ? 0 : 1) : (fail_n == 0 ? 0 : 1);` 修正。
3. 全量 `bash flint-vm/build.sh` 退出 0：所有单测 + 全部 `--sabotage` 对照组 + ISA 对拍通过。

### 8.8 第 14 轮：UI 前卡点收敛到 rc.sysinit 的 rptun/mount 段

2.5G 跑（DSI+充电器修复后）末态与旧 2.2G 跑完全一致：
UART 尾 = `yaffs: 0 blocks to be sorted...` + `offline_log [9:100]` + `log_service [11:100]`，
**没有 kvdbd/miwear/bluetoothd 的 `[pid:prio]` 打印**。

p65 的 rc.sysinit 顺序（原文见 §8.6）在 `offline_log` 之后是：

```
rptun start /dev/rptun/sensor        ← p65 特有（两个 rptun 对端核）
rptun start /dev/rptun/audio
mount -t rpmsgfs -o cpu=sensor,fs=/proc /mnt/sensor
kvdb_init
kvdbd &                              ← 该出现 kvdbd [pid:100] 而没有
```

**卡点收敛**：stall 在 `rptun start sensor/audio` 或 `rpmsgfs mount` 或 `kvdb_init` 之一
（对端核在模拟器里都不存在）。下一步探针（已备好地址）：
- `0x0C412C58` = rptun 内建函数（它的小字面量池里有 "rptun" 串与两个 PSRAM 自指结构
  `0x3C3AFC04/0x3C3AFC0C`，结构体即 rptun 设备对象）→ 挂 site 看 builtin 有没有被叫。
- `0x0C4F41E6` = nsh 的 rc 脚本运行器调用点（file 0x4F41E6，r1=rcS、r2=rc.sysinit 两个串的包装）。
- 判据仍是"UART 出现 `kvdbd [pid:100]`"。

**另一个已知不在关键路径上的**：0x40082000 门铃族（p65 每 2000 拍写 0x1ACCE551，
p67 文档 §74 判为"核间请求应答位"，p65 1.3M 次写里读回不判——p65 的版本是**不检查回读**的，
所以不是它卡；但 rptun 若也走这个窗口，建对端时顺带覆盖）。

### 8.9 第 14 轮：两个候选地址站点全零命中 + 下一步探针锚定

2.2G 探针跑（`build/p65_scan/rptun_probe.log`）：
- `--site 0C4F41E6`（我以为的 nsh rc 运行器）**0 命中** —— 地址读错了或那条路不走；
- `--site 0C412C58`（我以为的 rptun 内建）**0 命中**；
- 日志站点 212 条（与旧跑同级）—— 系统行为与旧跑一致。
- 0x40082xxx 窗口（PC 0C9746xx）：+0x82C00 读1.66M/写3.32M，+0x82000/+0x82008/+0x8200C
  **纯写**（553K-1.1M），与"每 2000 拍盲写、读回不判"一致 —— **不是等待循环**。

**下一步探针（已钉住锚）**：任务生成打印 `"%s [%d:%d]\n"`（file 0x1C4374，NSH 的 `&` 后台启动分支）
的代码在 **file 0x6AEA24（XIP 0xC76AE24）**。下一跑挂 `--site 0C76AE24`：
每次后台任务生成 = 一条命中，最后一条 = rc.sysinit 走到的最后一个 builtin，
再看该跑的末态 PC 是哪个线程 —— 卡的就在它后面。
另可用 `--uart-stop kvdbd`（UART 出现该子串即停并 dump）做二值判据。

### 8.10 第 14 轮定案：kvdbd 在全 2.2G 内从未出现（二值判据）

- `--uart-stop kvdbd`（UART 出现该子串即停机）：**跑满 2.2G 上限、从未触发** ⇒
  **kvdbd 任务从未生成** ⇒ rc.sysinit 的 stall 位置锁定在
  `rptun start /dev/rptun/sensor` / `rptun start /dev/rptun/audio` /
  `mount -t rpmsgfs -o cpu=sensor,...` / `kvdb_init` 四步之内（都在 kvdbd 之前）。
- 任务生成打印 `[pid:prio]` 的站点猜址（0C76AE24）两跑零命中 —— 该格式串的代码位置
  仍未钉准（不要再按"字符串→字面量→ref"单跳推地址，本轮连错两次；下一轮改用
  Ghidra 的 xref 对**数据引用**直接查，或换用 uart-stop 这种二值判据打头阵）。

**下一轮第一枪（按此顺序）**：
1. Ghidra `xref` 查 `"%s [%d:%d]\n"`（PSRAM 0x3C1945B0）的数据引用 → 得真正的打印函数 →
   site 它 → 每次后台任务生成一条命中，最后一条 = stall 前最后一个 builtin。
2. 同轮挂 `--site 0C4F41E6 0` 之前的 rc-runner 疑点用 Ghidra 对 0x4F41D4 反编译求证
   （该函数是否确为 rc 脚本运行器；两站点零命中说明这两个地址至少有一个是错的）。
3. rptun 对端（sensor/audio）需要建模时：precedent 是 p67 文档 §74.4 的"对端应答器"
   四步建法；p65 的 rptun 设备对象已在 PSRAM 定位（0x3C3AFC04 族自指结构，
   builtin 池 file 0x412C4C 引用 "rptun" 串）。

### 8.11 第 15 轮：rc.sysinit 时间线与 rptun 卡点定案（milestone 探针）

**内建表全解出**（任务表 file 0x1B5300 起 {handler, name, prio, stack}；
NSH 命令表 file 0x1C4E80-0x1C5180 {name, handler, flags, help}）：
kvdb_init=0x0CB943A9 / kvdbd=0x0C61EE59 / miwear=0x0CA393B9 / offline_log=0x0C5AD811 /
bluetoothd=0x0C605131 / mount=0x0C5B13AD / rptun=0x0C5B47E1。

**站点实测（全部带拍数）**：
| 拍 | 事件 | 站点 |
|---|---|---|
| 1.535,7G | 0C5AD810（offline_log 相关主）首次执行 | lr=0C4FD673 |
| 1.536,0-1.536,4G | mount ×3（proc/tmp/resource） | lr=0C5AE34B（NSH 分发器） |
| 1.751,6G | mount ×1（/data 的 yaffs，耗时 215M 拍） | lr=0C5AE34B |
| 1.859,9G | UART 首现 "offline_log" 打印（`offline_log start -w &` 的任务横幅） | — |
| 1.860,0G | **0C5B47E0（rptun 内建）调用一次** | lr=0C5AE34B |
| 之后到 2.2G | **再无第二次 rptun 调用（audio 那行没跑到）** | — |

**判读**：shell 在 `rptun start /dev/rptun/sensor` 里**没有返回**（0C5B47E0 只进过一次，
脚本下一行的 audio 调用从未发生）⇒ **卡点 = rptun start 内建内部**。
且 1.86-2.2G 间无任何 MMIO 窗口的"最近 PC"落在 0x5B4/she'll 区 ⇒ 等的是内核对象（信号量），
不是寄存器轮询 ⇒ 对端核（sensor/audio）在模拟器里不存在，其 start 握手无人应答。

**下一步**（按序）：
1. 读 0x5B47E0 → rptun_start 的等待原语（哪个信号量/标志位，谁负责 post）；
   rptun.c 已见至少两处（file 0x412B90 区的列表遍历 + "rptun" 串的断言）。
2. 按 p67 §74.4 的先例给"对端应答器"建模（四步：profile/test/sabotage/wire），
   最小语义 = 在 start 握手处把等待对象 post 掉 / 把状态字节改成"就绪"。
3. 卡点解除后的判据：UART 出现第二次 rptun（audio）→ kvdb_init → `kvdbd [pid:100]`
   → rcS 应用清单（bluetoothd/miwear）→ miwear main → 首帧。

### 8.12 第 15 轮：crt0 多段装载机制（纠正此前单映射认知）+ wstop 负结果

**wstop 探针**（`--wstop 3C3AFC04 100` → 首次命中在**第 10,542,340 条、PC=0C0C034E**，
即 crt0 的搬运循环里；`--wstop 3C3AFC04 100 1900000000`（1.9G 后才武装）→ 全程无命中）：

1. **0x3C3AFC04 的初值由 crt0 拷贝写入** —— 它是镜像里的静态数据，不是运行期对象分配。
   命中现场：PC=0C0C034E（crt0 搬运循环内，file 0x31A-0x352 那个四指令环），
   LR=0C0C003B（crt0 本体），寄存器 r0=3C3AFC04(值) r1=2C4C1E44 r2=3C3AFC08 r3=2C49F9CC。
2. **"file = PSRAM 偏移 + 0x2FDC4"只对字符串段那一份拷贝成立** —— 这里观察到
   另一份 blob：源在 file 0x401xxx 一带、目的在 PSRAM 0x3C3AFC04 一带，
   与 0x2FDC4 口径算出来的目的（0x3C3D1E80）**不是同一段**。crt0 是**多段装载**，
   要搬整段得逐段读 crt0 的装载表（此前在 file 0x36C 池里见过 (src,dst) 对：
   0x2C0EFDC4→0x3C000000、0x2C4C1E44→0x3C3D2080 等，与本次观察同一族）。
   **修正 §7.4 的措辞：那条映射只用于"查字符串在哪"，不能当整镜像线性公式。**
3. wstop 不命中 ⇒ rptun start 的等待**不写这个结构区**（等的是别处的信号量/状态），
   与"1.86-2.2G 无 MMIO 访问"互相印证：**纯内核对象等待，对端核未建模**。

**下一步（rptun 对端建模，工作块交给下一轮）**：
- 入口件已齐：rptun 内建 handler 0x0C5B47E0（含其余 rptun 引用的函数在 file 0x412B90
  一带的 rptun.c）；rptun 设备对象族初值来自 crt0 搬运的 file 0x401xxx blob。
- 需要读出的三件：`rptun_start` 的等待原语（哪个信号量）、谁负责 post、
  对端核的 mailbox/ring 寄存器窗口（找不到 p65 版 "mcu2sens" 字符串，通道命名待考）。
- 建法与 DSI 同规格：profile 区域/键 → 设备 + 单测 + sabotage → test_boot 接线 → 实跑验证
  （判据：第二次 rptun 调用出现 → kvdb_init → `kvdbd [pid:100]` → rcS 应用清单）。

### 8.13 第 15 轮：rptun 对端链路词汇全档（下一工作块的地图）

字符串群定名（file 0x306B80 / 0x313640 / 0x308B3C）：

| 词条 | 含义 |
|---|---|
| `mcuc0_2_sensc0.c` / `[RPTUN] [MCUC0_2_SENSC0]` | 传感器对端链路的编译单元与日志前缀 |
| `[RPTUN] [MCUC0_2_MCUC1]` | 第二条对端（MCU C1）；audio/DSP 用 `rptun_ops_ap2dsp_start` |
| `rptun/chips/best1502p/rptun_ipc_impl.h` | 平台层实现目录（best1502p 自有 IPC 实现） |
| `hal_rmt_ipc_open` / `_get_ep` / `_rx_irq_handler` / `hal_rmt_ipc_start_recv` | RMT IPC 层：open / 取端点 / 收中断 / 起收 |
| `%s-OPEN:%d: rxPtr=(n/a) txPtr=%p manualRxDone=%d` | OPEN 日志（txPtr 打印出来） |
| `setup vring buffer time out %d` / `rptun_setup_rsc_table timeout:%d` / `rsc_addr %x` | **等待原语：资源表/vring 交换超时** |
| `force panic sensor ...` / `force panic audio ...` | 对端 panic 注入测试口 |

运行期实测：rptun start（1.8600G）当场只有两条 `denug log call %p`（lr=0C964BCF/0C964B79，
打印的指针 0C4D294B / 0C964BDD），之后 340M 拍零日志、零 MMIO、零该结构区写。

**判读**：卡点 = `rptun_setup_rsc_table`（等对端资源表），对端核不在，表永远不出现。
**下一轮建模路线（明确的三件）**：
1. 读 `rptun_setup_rsc_table` 的实现（字符串 refs：`rsc_addr %x`、`setup vring buffer time out`），
   拿到**资源表期望地址**与轮询/等信号的机制；
2. 找到 `hal_rmt_ipc_open` 的 txPtr 出处（共享内存窗口），确认对端资源表应放哪；
3. 用 `--memfill <rsc地址> <长度> <值> <第N条>` 先做**零代码验证**：把一张最小合法资源表
   预置进共享内存再跑（若 timeout 消失、第二次 rptun 出现，就证明方案成立），
   随后按四步规矩把这套预置固化成设备/档位。

### 8.14 第 16 轮：rptun 等待解剖（函数级）

**等待链（全部读自反汇编，file 偏移）**：

```
rptun start 内建（0x0C5B47E0）
  → rptun_start 主体（file 0x8A4F44）：
      get-cfg(core=1, id=1)            file 0x8B33AC → 0x8AB288
        └ 0x8AB288：core==2 → cfg=0x20195D50（SRAM）；core==1 → cfg=0x3C3CECA4（PSRAM）
        └ cfg 布局：count=byte[cfg+0x34]（=2）；数组=word[cfg+0x38]（=0x2019E410）；条目 0x30 字节
        └ id=1 的端点结构 = **SRAM 0x2019E440**
      “open peer” 等待环（file 0x8B3400）：100 × delay(10)
        └ 就绪判据 = **byte[端点+0x2F] != 0**（file 0x8B3396 只读这一字节）
        └ 超时打 “Error: IPC can not open peer %p”（file 0x8B341A）并返回 -1
      rsc/vring 设置（file 0x8A50E4，内含 1000 × delay(10) 环）
        └ 退出条件：[dev+0x64] != 0 且 [dev+0x78] != 0（dev = 链上传下来的结构）
        └ 超时打 “setup vring buffer time out %d”；成功也打 “rptun_setup_rsc_table timeout:%d”
        └ 这两个 log 走 **file 0x8B45B8**（不是 0x0C504A54 那条），所以日志站点看不到它们
```

**时基换算**：每档 delay(10) 在模拟时基 ≈ 25M 拍（“2.5G 拍 ≈ 1 秒”的旧标定），
100 档 ≈ 2.5G 拍、1000 档 ≈ 25G 拍 —— 与"1.86G 起在 2.2G/4G 都还在等"完全吻合。
**结论：自然超时不可等；正确路线是把就绪条件预置出来。**

**解锁实验设计（下一跑）**：`--memfill 2019E46F 1 01 1900000000`
（1.9G 时把端点就绪字节置 1）→ 看第二次 rptun 调用是否出现。
若第二环还拦，再用运行期 dump 定它的 [dev+0x64]/[dev+0x78] 落点。

### 8.15 第 16 轮：两处修正 + rptun 停点收紧

**修正一（重要）：file 0x1B53xx 那张表不是"处理函数表"。**
站点实测 0x0C5AD810（表里"offline_log"的 field0）反汇编 = 一段 64 位乘累加数学
（umull/adcs 链），不是任何 app 的 main。⇒ 该表的 field0 不是可调用入口；
此前 §8.11 里"offline_log=0x0C5AD811"这类映射**全部作废**，别再拿它们下结论。
（对照：NSH 命令表 file 0x1C4Exx 的映射是**实站点验证过的**——mount=0x5B13AC 在
1.536-1.75G 命中 4 次与脚本的 4 个 mount 对齐；rptun=0x5B47E0 在 1.8600G 命中——
这张表继续用。）

**修正二：调用栈跟踪器的"最深帧"不可当停点证据**（48 层上限 + 98M 次溢出的统计
已经点名它：0x5AD810 是热函数反复进出，不是"1.5357G 进去没出来"）。

**rptun 停点当前证据（收紧后）**：
- rptun 内建（0x0C5B47E0）argv 实测 = `rptun / start / /dev/rptun/sensor`（站点+site-mem
  dump 三个字符串指针，6 字节间距，坐实）。
- 1.8600G 进入一次、到 4G 没有第二次（audio 那行没跑到）⇒ 卡在这一次调用内。
- 但候选内部点**全部零命中**：rptun_start 主体 file 0x8A4F44（含 get-cfg 调用
  0x8A4F78）、1000 环（file 0x8A518A）、100 环（file 0x8B3410）都没执行。
  ⇒ 阻塞位置在这些点**之前**：最可能是 builtin 里的 open()/驱动路径或早期信号量等待
  （不是那两个 delay 环——预置端点就绪字节 2019E46F/2019E43F 无效也与此一致）。

**下一轮探针（已定）**：
1. 用 `n_mcuc0_2_sensc0.c` 字符串（file 0x306CC2）的代码引用定位传感器链路驱动，
   挂 site 找 open 路径的等待点；
2. 备选：把 rptun 内建 0x5B47E0 的入口字节流从 0x5B4700 向前扫到函数序言，
   直接在它的体内逐段 bisect（site 每 0x20 一次，看最后命中到哪）。

### 8.16 第 17 轮：rptun 链路全解剖（两条 start / 两张 cfg / 端点就绪字节）

**两条链路各自的 start 函数**（结构同型，读自反汇编）：

| 链路 | start 函数 (file) | get-cfg 调用 | 端点来源 |
|---|---|---|---|
| sensor（mcuc0_2_sensc0） | **0x8A4BC0** | `r0=2, id=0` → cfg=**SRAM 0x20195D50** | arr=0x2019E4D0 + 0x30*0 = **端点 0x2019E4D0** |
| mcuc1（MCUC0_2_MCUC1） | 0x8A4F44 | `r0=1, id=1` → cfg=**PSRAM 0x3C3CECA4** | arr=0x2019E410 + 0x30*1 = 端点 0x2019E440 |

cfg 布局（两处同）：count = byte[cfg+0x34]（=2）、数组 = word[cfg+0x38]、条目 0x30 字节。

**sensor start 的执行链**（全在 file 0x8A4BC0 起，站点+lr 实测进入时刻 1.8601G）：
```
log → bl 0x8A8F14（hal_rmt_ipc 层） → bl 0x8A49B8
→ get-cfg(2,0) → bl 0x8B3400（open 等待环：100 × delay(10)，就绪=byte[端点+0x2F]）
→ bl 0x8A50B6 → bl 0x8A51D8 → bl 0x8A50E4（rsc 等待环：1000 × delay(10)，条件 [dev+0x64]&[dev+0x78]）
→ log “rsc_addr %x”
```
**rptun.c 调用 start 的路径**：file 0x412946 经 ops 表 `blx r3`（[r0]→+0x18）调到 0x8A4BC0；
rptun_executor 侧的“命令执行”在 file 0x5B47xx（**不是内建本体**——它是 NSH 命令/脚本
执行器，argv 实测 {rptun,start,/dev/rptun/sensor}）。

**解锁实验 v2（进行中）**：`--memfill 2019E4FF 1 01 1900000000`（这次打的是真端点
0x2019E4D0 的就绪字节 +0x2F）。上一版打错了链路（core1 的 0x2019E46F）所以无效。
判据：站点 0xC973410（open 环）先命中若干次、1.9G 后停；`0xC964C04`（open 之后的
下一调用）出现命中。

### 8.17 第 17 轮续：sensor start 入口后全链 + IPC 操作解剖

sensor start（file 0x8A4BC0）**实测确实到达**（站点 PC=0xC964BC0，lr=0x4D294B=file 0x412946
的 rptun.c ops 分派），且"未就绪分支"也走了：两条 `denug log call`（lr=0xC964BCF/0xC964B79）
= 入口日志（bl@0x8A4BCA）与 err-log 助手（0x8A4B68，未就绪时打）的返回点。

入口之后全链（逐条读过）：
```
bl 0x8A8F14  → thunk 0x8B338E："返回 byte[*0x3C48E7B0 + 0x2F]"（ipc 对象就绪位）
bl 0x8A49B8  → b.w 0x8A8F50 → b.w 0x8B3388（跳转桩，预置 r3=0）→ 0x8B32D0
    0x8B32D0 = RMT-IPC 端点操作（file 0x8B32D0）：就绪位=byte[obj+0x2F]；
    未就绪返回 2 不阻塞；就绪则把 {opcode,arg} 写进 EP 槽位、调 [obj]+0x10 的 notify、
    并按 byte[obj+0x2A] 的线号在 [0x2019E5B4] 置位（0x2019E5B4 就在端点池 0x2019E4D0 之后！）
bl 0x8B33AC  → get-cfg(core=2, id=0) → 端点 = SRAM 0x2019E4D0
bl 0x8B3400  → open 100×delay(10) 环（就绪=byte[端点+0x2F]）——站点 0xC973410 **零命中**
```
**事实与疑点**：全线无反汇编可见的阻塞点，但 open 环站点一次不中、其后一切站点也不中。
⇒ 要么被测站点地址又踩进指令中间（本轮第二次），要么流程停在 0x8B32D0 内的内核调用
（`bl 0x7FEB60` / `bl 0x7FD980` / `bl 0x7FCEB0` —— 0x7Fxxxx 是跳进 **SRAM 内核**的 veneer 区）
或 0x8B3346 的 `bl 0x7FCEB0(5)`。**下一轮第一枪：站点排 0x8B32D0 各调用点 +
在 0x7FEB60 的目标（SRAM 内核 veneer 解出真实地址）上落站点。**

**另记**：0x2019E4D0 起的 0x30×2 端点池 + 0x2019E5B4 状态字 = 静态 SRAM 区；
0x3C48E7B0 = PSRAM 里的"ipc 对象指针"全局。这两处是后续"对端建模"最少要碰的写点。

### 8.18 第 17 轮定案：卡点在**日志路径**，不是 rptun 握手本身

**修正**：8.17 的"下一枪"里我转写地址时丢了一位（0C8B32D0/0C8A4BCA 是无效地址）；
改成正确 XIP 后（file 0x8B32D0→**0C9732D0**、0x8A4BCA→**0C964BCA**）站点命中如下：

```
B2 跑（sensor-start 内部，边界对齐）：
[0C964BCA] = 1   ← 入口日志的 bl（file 0x8A4BCA）
[0C964BCE] = 1   ← get-ready 调用（file 0x8A4BCE，bl 0x8A8F14）
[0C964BE2] = 0   ← ipc-op 调用（file 0x8A4BE2）—— 再没往前
其余全 0
```
入口与 get-ready 都执行了；**卡死点在 0x8A4BCE 与 0x8A4BE2 之间**。这段里只有两件：
get-ready 访问器（0x8A8F14→0x8B338E，6 条指令，不可能阻塞）和**未就绪分支的 err-log
`bl 0x8A4B68`（file 0x8A4BD8）**。而 0x8A4B68 内部的 bl（file 0x8A4B74）正是全系统
最后两条 `denug log call` 之一（lr=0xC964B79）——**日志助手开始执行后没有返回**。

**新判读（据此前的全部"日志站点在 1.8601G 后归零"现象一致佐证）**：
**真正堵塞的是日志子系统**——rptun start 走到"对端未就绪"分支去打一条错误日志，
这条日志（或日志路径本身）在 1.8601G 之后**再没有返回**，于是全系统的日志与推进
都静默了。rptun 对端缺失只是把流程引到了那条 doomed 日志上；即使把对端建出来，
只要日志路径仍会锁死，别的错误路径也一样会卡。
（本站一轮的头号候选：日志环锁被 log_service/offline_log 系统持有，或日志下游
"刷到 UART/落盘"的等待无人应答。）

**下一轮第一枪（已定）**：
1. 反汇编 `0x8A4B68`（err-log 助手）→ 顺着到日志函数（0x0C504A54 族）的锁定点；
2. 同时站点排"日志函数内部"的分段（0x0C504A54 之后的几条关键指令），
   看它到底停在哪一段：取锁 / 写环 / 通知 log_service；
3. 找到锁的持有者后，先判断是"等对端 core 把日志环读走"还是"等 log_service 任务"——
   这两条的解法不同（前者=对端建模，后者=可能只需把某个就绪信号预置）。

### 8.19 第 17 轮补：日志助手 0x8A4B68 的后半 + 0x8A8DFC 的"sensor 核 RAM 探测"

0x8A4B68（err-log 助手）后半：
```
bl 0x444A54（日志，**实测返回了**——两条 denug 之一就是它的返回）
bl 0x8A8DFC        ← 下一个调用，此后无任何回痕
```
0x8A8DFC（file 0x8A8DFC，hal_rmt_ipc 区，字符串 0x308B3C 族）：
```
kernel 三连（bl 0x16F4 / 0x7FD528 / 0x7FDDB0）→ delay(2) → bl 0x7FC148
r0 = **0x22000000**（疑似 sensor 核的 RAM/窗口基址）→ bl 0x7FDE88（内核调用）
cmp r0,2 → r0>2 打错（串 0x3C2D8DD1/0x3C2D80C2）→ r0==0 走下一相
→ 读 16 字节描述符 [0x3C2D8DA0] → bl 0x7FBF48 → bl 0x8A7150(0x22000000, …)
```
**判读**：错误日志路径把流程引进了 **hal_rmt_ipc 的"sensor 核探测/配置"**；
其中的 `0x7Fxxxx` 全是跳进 **SRAM 内核**的 veneer，阻塞在内核的远端核机制里
（0x7FDE88 拿 0x22000000 做的多半是"核 RAM 签名/状态探查"）。日志本身不阻塞——
**卡点 = 0x8A8DFC 内某个 kernel 调用**。

**下一轮第一枪（已定，两件）**：
1. 站点排 0x8A8DFC 内部调用点（0x8A8E04/08/0C/12/16/1E）找首个不返的内核调用；
2. 试验 `--memfill 22000000 <小段> <采样值> <1.86G>`：把 sensor 核 RAM 窗头预置成
   非零签名（"对端已起"），看 0x8A8DFC 的探测是否直接放行——这条路若通，
   就是与 DSI/充电器同规格的"对端核应答器"最小模型。

### 8.20 第 17 轮收尾：停点最终收敛到 `bl 0x7FDE88(0x22000000)`

- 站点排 0x8A8DFC 内部（0x8A8E04/08/0C/12/16/1E **6/6 全命中**，各一次）
  ⇒ 流程确实穿过 kernel 三连与 delay(2)，执行到了 **file 0x8A8E1E 的
  `bl 0x7FDE88`（r0=0x22000000）**；其后（0x8A8E22 cmp 之后）无站点（未排），
  结合"此后全系统静默"，**停点 = 该内核调用（或它返回后的相邻几条）**。
- `--memfill 22000000 0x40 2 1860500000` 预置：**无效**（sensor-start 的 ipc-op 调用
  0C964BE2 仍零命中）——可能签名值不对、或 memfill 的填充语义（按字节 vs 按字）与
  探测所需不符、或阻塞发生在内核调用内部与 RAM 内容无关。
- 0x7Fxxxx = 跳进 **SRAM 内核**的 veneer 区：0x7FDE88 的真实目标要按 SRAM 影子源法
  （dump SRAM 代码段 → flash 搜字节 → file offset → 反汇编）解出，这是下一轮第一动作。
  SRAM 内核代码此前的已知锚点：0x36FB7A 的 delay 读环、0x2036FB7A 家族。

**下一轮（接续）**：解 0x7FDE88 的 SRAM 目标 → 读它的等待 → 判"等 ram 内容签名"
还是"等对端核握手对象" → 决定用 memfill 还是建"对端核应答器"（四步规格）。

### 8.21 第 18 轮：对端核应答器 v0（sensor 核窗口魔数预置）落地 + 首测

**实现**（三处，全部按项目规矩）：
1. SoC JSON：regions += `sensor_ram`（0x22000000+0x10000, kind=ram，附依据与"尺寸待核"标注）；
   devices += `sensor_ram {base, magic=0xBE57EC1C}`。
2. flint_profile.py：发 `sensor_ram.base/.magic` 两键（p67/o66 无此键则不发）。
3. test_boot.c：装配期把魔数写进窗口头（`预置 sensor 核窗口签名：22000000 <- BE57EC1C`）。
   依据：file 0x8A7150 读 0x22000000 的 4 字节与常量 0xBE57EC1C 比对（rptun sensor
   start 的"sensor 核镜像就绪"判据）；对端核不存在时读回 0。

**首测（2.1G，`build/p65_scan/magic_probe.log`）**：预置行照打；但推进站点仍零：
`0C964BE2`（sensor-start 的 ipc-op 调用）、`0C968E72`（0x8A8DFC 里 `bl 0x8A7150`）
均 0 命中，`0C5B47E0`（执行器）仍只 1 次。
⇒ 停点**收紧到 0x8A8E1E 与 0x8A8E72 之间**：候选 = 分类器后分支上的
`0x8A8E44 bl 0x7FEA90`（→SRAM/file 0x36D6）或 `0x8A8E54 bl 0x7FBF48`（→file 0x3388）
——两者都是 SRAM 内核 veneer，还没被站点覆盖过。

**下一枪（已定）**：站点排 `0C968E44 / 0C968E54 / 0C968E68 / 0C968E70` + 复查
`0C968E04..1E`（新构建下 0x8A8DFC 内部是否仍照跑）→ 命中图会给出到底卡在哪个
veneer；随后按影子源法（δ=0x20163AE8）读它的 SRAM 实体。

### 8.22 第 18 轮：停点再进一层——SRAM 内核"远端核管理"（file 0x2F64）

gap 探针（`build/p65_scan/gap_probe.log`，边界对齐）：
```
0C968E04 @1.86017G (入口) → 0C968E1E @1.86075G（分类器）→ 0C968E44 @1.86075G（bl 0x7FEA90，返回了）
→ 0C968E54 @1.86075G（bl 0x7FBF48）→ **此后 0C968E68/6C 零命中**
```
⇒ **停点 = `bl 0x7FBF48`（file 0x8A8E54）**。veneer 目标 = SRAM 0x20166E70 = **file 0x3388**
（影子源 δ=0x20163AE8，本轮 dump 8 个探针全部线性吻合）。

file 0x3388 = 适配器（push r0/r1/r2/lr → `bl 0x2F64`(r2=0,r3=1) → 尾返回）。
**file 0x2F64 = AP 侧"远端核管理"op 分发**（这条链第一次走到它）：
```
校验核号 ≤2（否则打 SRAM 字符串 0x2016fe80 族，file 0xC398 对应）
查核表 [0x20171960]（0x18 字节/核；表与状态字节 0x201719CC 都在 SRAM，file 0xB878/B8E4）
按参数分流：bl 0x3A1C / 0x3FC4 / 0x3C74 / 0x3CE4 / 0x3CC8 …（核初始化/复位/状态一组子调用）
```
⇒ **阻塞在其子调用链里的某一个**。下一枪：站点排 file 0x2F64 的子调用边界
（XIP = 0xC0F3A1C 之类：0x3A1C/0x3FC4/0x3C74/0x3CE4/0x3CC8 +0x0C0C0000），
看最后命中到哪个；命中图给出后按影子源法读那个子函数。
（本条链上的所有地址都是 **file 0x2xxx-0x3xxx 与 SRAM 0x2016xxxx 的一一影子**，
以后查这个子系统统一用 δ=0x20163AE8。）

### 8.23 第 19 轮：核管理链的别名口径 + 站点预算机制教训

1. **内核/SRAM 代码的站点必须挂别名**：这些代码在 CPU 上是**从 SRAM 别名
   （0x0036xxxx）执行**的，挂 XIP 地址（0x0C0Cxxxx）永远零命中——本轮第一版排扫
   八个站点全零就是这么来的。换算：别名 = file + 0x20163AE8 - 0x1FE00000
   （即 file 0x2F64→0x00366A4C、0x3388→0x00366E70）。改别名后全中。
2. **--site 的"次数"是全局共享预算，早期命中会把追踪器烧死**：预算 12 时，
   3.29M 拍处的早期 bring-up 命中 12 次就把追踪器关到运转结束——1.86G 的行为
   全部看不到（v2/v3 两轮"全零"的真正原因）。**要做"活到 1.86G"的观察，
   预算必须大于早期命中总数**（本轮改用 100）。
3. **早期 bring-up（3.31-3.32M 拍）实测**：核 0/1 的 bring-up 会**连续 53 次**
   调 file 0x3A1C（别名 0x00367504）后继续走完全链（8/8 站点早段全中）——
   说明早期对"核不在"是有界重试、能自行结束并继续的。
4. `*0x3C48E7B0`（ipc 对象指针全局）**实测 = 0**（dump 拿到同处 +4 字 = 0x0C94FEDD
   一个代码指针）——sensor-start 的 get-ready（读 byte[*0x3C48E7B0+0x2F]）因此
   必然失败、走 err-log → 0x8A8DFC（hal_rmt_ipc 初始化）→ core-mgr。
5. **--memfill 已升级为可重复**（8 条上限，各自带起拍点）——本移植后续
   "多字段预置"实验都要用它（改了 test_boot.c；全量 build 待重跑）。

### 8.24 第 19 轮：核 2 路径在 1.86G 的实测轨迹（生存式排扫）

预算保活后的两组排扫（`coremgr_probe5/6`，别名口径）实测核 2 的 start 链：

```
1,860,749,861  0x00366E70（适配器 file 0x3388，由 0x8A8E54 bl 0x7FBF48 进入）
1,860,749,866  0x00366A4C（core-mgr file 0x2F64，r0=2）
1,860,750,265  0x00367AAC（sub 0x3FC4）
1,860,750,621  0x0036775C（sub 0x3C74）
1,860,750,632  0x003677CC（sub 0x3CE4）
1,860,750,644  0x003677B0（sub 0x3CC8）
1,860,751,020  0x00367EC8（sub 0x43E0）
1,860,751,030  0x003678A4（sub 0x3DBC）
1,860,751,041  0x003679F4（sub 0x3F0C）← 最后一个已证到达
```
其后候选（别名全列，**注意别学我上一版算错 0x100**）：
`0x2CE8→0x00366DD0、0x2D00→0x00366DE8、0x441C→0x00367F04、0x4A4C→0x00368534、0x4DD0→0x003688B8`。
另有旁证：早期 bring-up 在 3.31M 处有 22 次 AAAA5555+flash_ctrl(40140000) 的轮询
（0x00368634 自返回环）——那是 flash 空闲轮询，与本链无关，别混。

**下一轮**：probe7（修正别名）看核 2 链停在 0x3F0C 之后哪个子调用；停点所在子函数再精读。

### 8.25 第 19 轮收束：停点进入 file 0x441C（核状态清理序列）

probe7（修正别名后）晚段（>1.86G）命中：
```
0x00366A4C(1,860,749,866 入口) → 0x003678A4(751,030) → 0x003679F4(751,041)
→ 0x00367F04 = file 0x441C(752,235) ← 最后一个晚段命中
（0x2CE8/0x2D00/0x4A4C/0x4DD0 的别名晚段皆零）
```
⇒ 核 2 链停在 **file 0x441C**（别名 0x00367F04）内部。0x441C 实测是一段
**核状态清理序列**：二十多条子调用（0x3B54/0x3A88(r1=0xAB)/延时 0xC0BC/0x3C48/
0x3D98(r1=0xEB)/0x3EAC/0x3C24/0x3C00/0x3A50/0x3A1C/0x399C/0x3AAC(0x66/0x99/0xE9)/
0x3B24/0x3B70…）+ 逐字节清 SRAM 每核状态数组（0x2017248C/83/86/80/20172494/201724A0
+ 核号索引）。

**下一枪**：站点排 0x441C 内部序列（别名 = file + 0x20163AE8 − 0x1FE00000 逐个算好），
最后晚段命中者即停点；其后按影子源法精读该子函数。
（教训合成一条：**file→alias 换算必须逐位算，本轮我犯了两个 ±0x100 的错误；
核对法 = 把 alias 再减 0x2FDC4 看是否落回 file 地址。**）

### 8.26 第 20 轮：停点解码到指令级——**端口空位计数永远为 0 的无限重试**

传感器时代（>1.8607G）的干净轨迹（`--site-after` 门控实测，probe10/11）：
```
1,860,749,828  0x367570 (file 0x3A88, r1=0xAB)
1,860,752,238  0x36763C (file 0x3B54)   ┐
1,860,752,262  0x367570 (file 0x3A88)   │ 0x441C 序列
1,860,756,634  0x367730 (file 0x3C48)   │
1,860,756,645  0x367880 (file 0x3D98, r1=0xEB)
1,860,756,660  0x367994 (file 0x3EAC)   │
1,860,756,698  0x3676E8 (file 0x3C00)   │
1,860,756,716  0x367538 (file 0x3A50)   │
1,860,756,741  0x36763C (file 0x3B54, 第二次)
1,860,756,765  0x367484 (file 0x399C, r1=0x201705A5 r2=4) ← 最后到达
（0x367594/file 0x3AAC 及之后全部未达）
```
**file 0x399C = 死等环**：`bl 0x38F0(core, ptr, count)` 直到返回 0。
**file 0x38F0 = "往核端口写 count 个字节"**：
```
r4 = [0x201700B8 + core*4]          ← 每核的"端口对象"指针表！
room = ([r4+0xC] >> 9) & 0x1F        ← 端口空位计数（bits[13:9]）
循环：若 room+起点==起点（即 room==0）→ 一个字节都不写、返回剩余数；
      否则逐字节：byte=*ptr++; [r4+8]=byte（写口）
```
**根因链**：`[r4+0xC]` 的空位计数为 0（对端核不存在、无人消费/初始化端口对象）
⇒ 0x38F0 每次直接返回 4 ⇒ 0x399C 无限重试 ⇒ 整条 rptun start 永不返回。
**这是全程第一次把"永不返回"追到一条具体指令与一个具体字段上。**

**下一枪（已定）**：dump 核表 0x201700B8（拿 core2 的对象指针）→ 找该对象 +0xC 的当前值 →
用可重复 memfill 把空位计数预置成非零（如 bits[13:9]=0x1F）→ 看 0x399C 是否放行、
rptun start 是否返回、UART 是否出现 kvdbd。

### 8.27 第 20 轮：端口对象表 dump 出真值——sensor 端口 = 0x50160000

`--ramdump 201700A0 0x40` 实测（1.95G 末态）：
```
[0x201700B8] = 0x40140000   ← core0 端口窗（= flash_ctrl 基址）
[0x201700BC] = 0x40500000   ← core1 端口窗（= flash1_ctrl 基址）
[0x201700C0] = 0x50160000   ← **core2(sensor) 端口窗**（0x40160000+0x10000000 — 每核一个视图）
```
⇒ 0x38F0 的"端口对象" = 每核的**消息端口窗口**；sensor 的写口 = 0x50160008、
空位计数 = 0x5016000C bits[13:9]。flint 里该窗落在通用 mmio 锁存（没人写过 ⇒ 读回 0）
⇒ 空位恒 0 ⇒ 0x399C 死等环。**实验（进行中）**：`--memfill 5016000C 4 00003E00 1860500000`
把空位预置成 0x1F 格，看 0x399C 放行、rptun start 返回、UART 出 kvdbd。
若成立，这一步就固化成 profile 预置（与 sensor_ram 魔数同规格的"对端应答器"组件），
并顺带把 core0/core1 两窗同法备好（早期 bring-up 也可能要用）。

### 8.28 第 21 轮：空位预置**定案放行**；站点语义修正 + "对端应答器"固化落地

**定案（对照实验，两方向都有证据）**：
- **无预置**（probe11，无 `--memfill`）：晚段站点轨迹走到 `0x00367484`（file 0x399C，第 1,860,756,765 条）
  **即止**，此后 339M 拍零命中；0x3AAC（别名 0x00367594）**一次都不中**。
- **有预置**（roomfix1/2，`--memfill 5016000C 4 00003E00`）：0x399C 中一次后，
  **0x00367594 连中 4 次**（1,860,756,831 / 894 / 919 / 1,860,777,228，r1 = 0xFF/0x66/0x99/0xE9）。
⇒ 预置 bits[13:9]=0x1F 确实放行了发送原语，rptun sensor 的"核状态清理/重启序列"
（file 0x441C）继续走完了它后面的四条端口命令。

**站点语义修正（重要）**：0x399C 是**函数入口**，重试环在函数**内部**（0x39AA → bl 0x38F0 → cmp/bne 回 0x39AA）。
所以"0x399C 只命中一次"**两种结局都会出现**（放行退出 / 原地死转都只有一次入口命中），
单看它区分不了。**放行与否的判据必须挂在序列的下一站**（0x3AAC，别名 0x00367594）：
它连中 = 放行；零命中 = 死转。（本轮 probe11 vs roomfix 的对照就是这么分辨出来的。）

**roomfix1 与 roomfix2 末态逐项相同**（异常 1,958,624 次 / SysTick 跳 12,376,140 次 / 停机 PC=0C5145A6），
说明单发与五发等效——预置在 186,050,0000 拍写下后一路保持到 1,860,756,765 拍，
中间没有任何人改写 0x5016000C。**这一字段可安全地boot 期预置**。

**放行之后的下一步**：0x441C 序列的最后一条命令（0xE9，第 1,860,777,228 条）之后，
执行**没有回到 XIP 侧**——beyond1 实验（`build/p65_scan/beyond1.log`：同样的 5 发预置 +
站点 0C968E58/6C/72/76/8C/98/9C/AE/D0 + 00367594）显示：XIP 侧 9 个站点**全部零命中**。
⇒ **新卡点在 0x441C 尾部 / core-mgr（file 0x2F64）的续段里**（0x305A 以下那串子调用：
0x4A4C / 0x4DD0 / 0x44F4 / 0x4AA8 / 0x2F28 / 0x4CC4 / 0x4580 / 0x4B14 / 0x2C74 / 0x5164 /
0x4878 / 0x451C），而不在 XIP 的 0x8A8DFC 里。
（0x441C 尾部本身无环：bl 0x3B24 只是清 [obj+4] bit0 忙位、六条状态字节清零、直落返回。）

**"对端应答器"固化落地（三件套，与 sensor_ram 魔数同规格）**：
1. `best1502_r0.soc.json` devices += `sensor_port`（base 0x50160000 size 0x1000，
   `status_presets { "0xC": "0x00003E00" }`，_note/_provenance 写全）；
2. `flint_profile.py` 发射 `sensor_port.base/.size/.preset`（带字对齐与窗内校验，越界即退）；
3. `test_boot.c` 的 `win[]` 名单 += `"sensor_port"`（装配期经 mmio 锁存窗 poke）。
**构建全绿**（12 单测 + 20 组对照组 + profile 56 项自检 + 新预置行打印确认）。
上述 memfill 开关从本轮起不再是必需——profile 预置即可。

**顺手记两个坑**：
- `build.sh` 只编 `flint-vm/build/test_boot.exe`；之前的 `test_boot_p65.exe` 是手工产物，
  改 `test_boot.c` 后**必须手动 `cp test_boot.exe test_boot_p65.exe`**，否则跑的还是旧二进制
  （本轮第一次验证就撞上：预置行没打出来，因为 p65 那份还是 04:35 的旧件）。
- `alias = file + 0x363AE8` 换算**不许心算**：§8.24 里 "0x2CE8→0x00366DD0" 与
  "0x2D00→0x00366DE8" 两条是错的（正确值 0x3667D0 / 0x3667E8）；其余条目核对无误。
  一律逐位算，拿已知样本（0x399C→0x367484、0x441C→0x367F04）对表。

**进行中（responder1）**：无 memfill、纯 profile 预置 + 16 站点（含 core-mgr 续段全部子调用边界
与 XIP 返程点 0C968E58/6C），目的：① 证明预置固化后放行仍成立；② 把新卡点收窄到具体子调用。

### 8.29 第 21 轮续：预置放行成立、卡点逐层推进到"内核期 assert→board_reset(2)"

**responder1（无 memfill、纯 profile 预置、16 站点）——固化验证成立**：
站点轨迹从"序列+core-mgr 续段"一路推进：
```
0x00367594(0x3AAC)×4 → 0x0036760C(0x3B24 清忙位)×2（0x441C 序列走完）
→ 0x00368534(0x4A4C) → 0x00368068(0x4580) → 停在 0x003680C0(0x45D8) 入口之后（0x45D0 零命中）
```
**file 0x45D8 = "读事务"**：先 `bl 0x39F6(core, 待读数)` 等"已收字节数 ≥ 请求数"
（file 0x3954 = `([obj+0xC] & 0x1FF) >> 4`），再逐字节读 +0x10。对端不答话时同样死等。

**responder3（预置值升到 0x3FF0 = 空位 0x1F | 已收 0x1F<<4）——读事务放行，整条链穿透**：
```
0x45D8 → 0x45D0(0x4588) → 0x4DD0(键表查找) → core-mgr 尾 0x315A/0x3B24
→ 0C968E58：adapter 返回 XIP 侧，r0=3（键表未命中，读回全 0 的应答）
```
**注意 0x4DD0 的语义**：把读回的 3 字节与 0x27 条键表（表指针 SRAM 0x201705AC）比对，
命中才能继续（`beq.w 0x31C6` 是 r5=3 的退出位）。我们的读回是 0x000000，必然未命中 ⇒ r0=3。
XIP 侧（file 0x8A8E58）对 r0≠0 走 err-log 后仍然**继续往下跑**（0x8A8E40 的 cbz 之后落 0x8A8E6C），
所以 r0=3 不是致命错误。

**放行之后内核真跑起来了（新设备活动，旧吸引子被打穿）**：
- SRAM 内核（别名 0x0036xxxx）对 **core1 端口窗 0x40500000** 跑了长会话：
  +0 写 0xEB 命令 99956 次、+8 逐字节压 0xC0 64899 次、+0xC 读 548329 次（0x3A1C 忙位门，位31=0 立即过）、
  +4/+10/+18/+1C 读写数十万次 ⇒ 这是**核间消息端口**（不是 flash 驱动；窗口与 flash1_ctrl 重合）。
- GPDMA 通道活动（+0x100/104/108/10C 各写 13431 次，搬向/自 SRAM 0x201969B8、源 0x40500008）、
  ISPI 事务（+0xC 读 21515 次）、cache 维护（0x23FFA018 窗口写 443223 次）、
  aon_timer0 +4 被 SRAM 内核（别名 0x0036FB92/7A）读 823 万次（时基/延时循环）。
- UART1 总写出仍是 2926 字节（无新里程碑），但内核态大幅推进。

**新的停机点 = 又一次 NuttX assert → board_reset(2)**（与 §5 早期那条同族，但这次是放行后才出现）：
- 停机 PC=0x0036EB36 = file **0xB04E** 的 `b .`（逐字节核过：该处字 =0x14E2E7FE，前一字节=0x14D92017，
  与 file 0xB04E/0xB052 逐字节吻合）。
- 进死环前的序列（park1，站点+栈 dump）：file 0xB030（关机序列 FUN_0000b030）在 1,863,120,268 被调；
  其体内 0xC0A8 延时循环中 10 次（1,863.6M-1,864.5M，每次延时 ≈ 97,647 条）；
  最后一次调 file 0xAA28（PMU 寄存器序列 0x3037/0x3048/0x304d/0x3036/0x3043 + 延时 0x80），
  其尾部 `bl 0xB9EA`（**__set_BASEPRI：uxtb r0,r0; msr basepri,r0; bx lr**）在 1,865,373,832 置 BASEPRI=0x80，
  之后 7 条指令即入死环（1,865,373,839）。末态 BASEPRI=0x80、挂起位图=0x8000（SysTick 被优先级掩住）。
- 复位分发器 = file **0x44EC4C**（=`board_reset` 族：先 log `reset mode:%d`（传 2）+ backtrace，
  再按 mode 分派；mode 2 分支 `bl 0x7FCBE0` veneer → **SRAM 0xB09B**（关机执行体），最后 `wfi;b .`）。
  引用者两处：file 0x40340C（在 FUN_00403330 = **NuttX up_assert 族**内）与 file 0x4373FA（另一包装）。
  ⇒ 本轮停机 = 新内核工作里又触发了一次断言（内容待 assert1 实验：挂日志入口 0x0C504A54 + r1 dump 取格式串）。

**顺带定案的换算/语义**（都写进工具注释，别心算）：
- 4 字节 BL 的 lr = 调用点 + 5（thumb 位在内）：`call_site = lr - 5`。
  样本：site 003680C0 的 lr=00368553 → 调用点 file 0x4A66（= `bl 0x45D8`，与反汇编逐字对上）。
- file 0xB9EA = `__set_BASEPRI`（uxtb+msr+bx lr）；file 0xAA28 = PMU 配置序列；
  file 0xB030 = 关机收尾（hw op + 延时 + PMU 序列 + `b .`）。
- 别名 0x0036EB36 ↔ file 0xB04E 的对照法：把 alias−0x363AE8 与镜像字节逐字核（本轮两次核准）。

**进行中（assert1）**：`--site-after 1863100000 --site 0C504A54 40`（日志入口）+ 两个 reset 调用点，
`--site-mem r1` 直接 dump 格式串 —— 让固件自己报出断言的文件/行/任务。

### 8.30 第 21 轮续二：撤销"board_reset 分发器"读法，停机重定性为"任务级复位收尾"

**assert1（挂日志入口 + 两个复位调用点）零命中，推翻 §8.29 里两条推断**：
- 站点 `0C504A54`（日志函数 file 0x444A54，va_args 转发）、`0C4373FA`（FUN_0044ec4c 的引用点之一）
  在第 18.631 亿条之后**全部零命中** ⇒ **file 0x44EC4C 那个分发器根本没跑**，
  "这一次是走 board_reset(2) 分发"的说法作废。
- 根因是**把栈里的陈旧残留当调用链**：park1 的栈 dump 里 `0C504A6B`/`0C50ECFB` 看着像返回地址链，
  但按它们反查的函数没有执行过。**纪律：栈内容不等于调用链**——除非同一时刻的站点/日志证明它在动，
  否则栈上任何"看起来像返回地址"的字一律标为待证。（§8.15 修正一/二 是同一类错，第三次犯。）

**重定性（有据）**：
- park1 的现场：站点 `0x0036EB18`（file 0xB030 **函数首指令**）在第 1,863,120,268 条命中，
  lr 是陈旧的（`bl 0xB9EA` 的返回址），且进入方式是 `pop {pc}` —— **函数首指令被当作 PC 恢复**，
  这是**任务上下文启动**的形态（NuttX 任务首次被调度时，异常返回把栈里的 entry 灌进 PC）。
- 该任务体（file 0xB030-0xB04E）做的事：`bl 0x6558`（hw op）→ 延时 0x50 → `bl 0x6558` →
  **10 次延时 0x10**（站点 0x0036EB28 连中 10 次，每档实测 ≈ 97,647 条）→
  `bl 0xAA28`（PMU 序列：ISPI 选择子 0x3037 清 0x180、0x3048 写 0x2000、0x304d/0x3036 写 2、
  0x3043 置 0x6000，再延时 0x80，日志 tag 0x4040）→ 尾部 `bl 0xB9EA` 置 **BASEPRI=0x80**（1,865,373,832）→
  7 条指令后进 `b .`（1,865,373,839）。
- 末态自证：BASEPRI=0x80、挂起位图=0x8000（SysTick 挂着但被优先级门挡住）、IPSR=0、
  SysTick 跳数不再增长 ⇒ **这是"写完 PMU 就等芯片复位"的收尾环**，与 §5 早期
  `PMU 掉电序列 + 死循环等软复位` 是同一形态，只是这次由任务上下文进入。

**接法 = 用模拟器已有的 --reboot 机制**（不是新造）：`test_boot.c` 的复位停机点判据是
"**同一个 PC 连续执行 >200000 步**"（不是"跑了很多条指令"），命中后 `fl_bus_free`+`fl_bus_init`
重建总线 → **重跑 wire_devices（含 profile 的 cmu/mcmu/sensor_port/sensor_ram 预置）** →
`cpu_reset(entry, sp_boot)` 从头再起。p67 时代用它打通过 file 0x29C48C 那一格。
AP 引导链本来就"复位一次再起来"，所以 p65 这一格按同一口径处理。
**实验（进行中，reboot1）**：`--deep 6000000000 --reboot 4 --uart-stop kvdbd`，
站点排 `0C5B47E0`（rc 执行器）/`0C968E58`（adapter 返回）/`00367594`（sensor 序列锚）。
判据只看两件事：**UART 是否出 kvdbd**、**rc 执行器是否在第二次引导里命中**。

**工具坑（对外发布）**：`scripts/publish-module.sh` 的"推送成功"判定用 `ls-remote` 比哈希，
代理抖动时 8 次全部判失败并锚定分支，但**其中一次实际已推上去**——手工
`git push module module-publish-<mmdd-hhmm>:master` 立刻 `12d71a8f..31e6d75f` 成功。
遇到"8 次全失败"先手工推一次并核对 `git ls-remote module refs/heads/master`，别急着重跑脚本
（重跑会在 module/master 上再生成一个同内容提交）。

> 勘误与索引：本文逐条核定过的 p65 地址（端口族、核管理、DSI、rc 命令、复位收尾族）
> 已同步到 `docs/fw_address_table_p65.csv`，每行带证据等级；
> p65 没有官方符号包（`canopus_src/targets/` 只覆盖手环各版本），
> 所以那张表是唯一按"静态核对 + 模拟验证"两级标注的 p65 地址清单，别再往本文里塞散落的地址。

### 8.31 第 21 轮收束：断言取证定案 —— 所谓"核 2 消息端口"其实是第三颗 flash 控制器

**固件自己把话说出来了**（站点在断言包装器入口命中，r0/r1/r2 直接给出消息与行号）：
```
[site 0C4F74E8] 第 1,860,830,742 条 lr=0C968E41｜r0=3C2D8DC1 r1=00000053 r2=3C2D8113 r3=0A
```
`lr=0x0C968E41` → 调用点 file 0x8A8E3C；串池解出：
`r0 = "%s: Failed to open flash: %d"`、函数名 **`sensor_hub_open`**、断言条件 **`ret == HAL_NORFLASH_OK`**、
行号 0x53；同一函数里还有 `id < HAL_FLASH_ID_NUM`（行 0x41）与 `%s: addr not in flash: 0x%08X`。
⇒ **停机的真因 = `sensor_hub_open` 打开"第三颗 NOR"失败后断言**，断言处理器（file 0x403330）
再调 `board_reset(2)`（0x0C4C355C→0x0C50EC4C 连着命中，r0=2），落进等复位的收尾死循环。

**两条前一轮的推断作废（纪律 18，显式写出来）**：
1. §8.29 说的"电源键松开导致关机"不成立：那条路径（file 0x8B4ED8）要求 boot-mode 变量
   `[0x20173D92] == 1`，`--watchr` 实测该格从 crt0 清零后**全程为 0**，且 0xC950BF0/0xC974ED8/
   0xC974E5C 三处站点零命中。字符串 `pwr_key rel, shutdown!!!` 只是同一簇里的另一条不相干日志。
2. §8.30 说的"复位分发器没跑"也不成立：`assert1` 那枪把门限设在 1,863,100,000，而分发器跑在
   1,862,036,530 —— **门限之后的零命中是假阴性**。判"某段没执行"必须把门限放到它之前。

**语义更正（这条链上最值钱的一条）**：`[0x201700B8 + id*4]` 不是"每核消息端口对象表"，
而是固件那张**按 flash id 索引的控制器基址表**，三项 = 0x40140000 / 0x40500000 / **0x50160000**。
所以"端口原语"就是 flash 控制器的 FIFO，字段与 `flint-vm/src/l2/dev/flash.c` 早已有的形状逐字对上：
`+0x8` 发送槽（逐字节压）、`+0xC` bits[13:9] 空槽数 / bits[8:4] 本次应答剩余字节、
`+0x10` 取走一字节并推进、`+0x4` bit0 起始位、`0x9F` 读 JEDEC ID。
那张"39 项键表"= 固件的已知 flash ID 表（运行期 SRAM 0x201705AC，源表 file 0xC5DC 一带，
本地那颗 `C8 60 17` 就在表里 —— 这也是为什么 id=0/1 一直是好的）。

**修法（撤掉权宜，接真设备）**：
- 删掉上一轮的 `sensor_port` 伪造预置（SoC 条目、flint_profile 发射块、test_boot 的 `win[]` 三处）；
- 新增第三颗控制器 `flash2_ctrl`（region + device，`inherits: flash_ctrl`，器件 ID 暂退回
  `C8 60 17`，等级=推断待核），在 `test_boot.c` 里按 flash1 的同一形状接上并报名字；
  profile 生成器对这颗做了"存在才发射"的处理，别的目标不受影响。
- 全量构建通过（12 单测 + 20 组对照组 + 两个 CI 门 + 三份 p65 profile 门）。

**判据（flash2_run 在跑）**：不带任何预置、不带 `--reboot`，看
① 0x0C968E58 那格的返回值是否从 3 变 0；② 断言哨兵 0x0C4F74E8 是否还命中；
③ rc 执行器 0x0C5B47E0 是否在 1.86G 之后再命中；④ UART 是否出 `kvdbd`。

**方法教训（合成一条）**：这条链上我连错三次（栈残留当调用链、门限假阴性、把 flash FIFO 当 IPC 端口），
共同病根是**先给窗口起名再找证据**。以后遇到"逐字节压 + 状态里报空槽/应答剩余 + 读一字节走"这种形态，
先拿仓库里已有的设备字段布局去对（`flash.h` 那份注释就是现成的判据），对上了就不许再造新设备。

### 8.32 第 22 轮：接上第三颗控制器**成立**——断言行号从 0x41/0x53 推到 0x59

`flash2_run.log`（不带预置、不带 `--reboot`，2.4G 拍）四条判据逐条对：

| 判据 | 结果 |
|---|---|
| ① `0x0C968E58`（`bl 0x7FBF48` 之后那一格）的返回值 | **3 → 0，成立** |
| ② 断言哨兵 `0x0C4F74E8` | 仍命中 1 次，但行号变成 **r1=0x59**；lr=`0x0C968E41` = `sensor_hub_open` 里 `bl 0x4374E8`（file 0x8A8E3C）的返回址，`call_site+5` 逐字对上 |
| ③ rc 执行器 `0x0C5B47E0` | 0 命中（还没到 rc） |
| ④ UART 出 `kvdbd` | 没有；2850→2926 字节仍停在启动横幅段 |

站点 `0x00367594`（file 0x3AAC 发送放行点）在 1.8607G 命中 5 次，最后一次 lr=`0x00368521`；
r5=**2** 就是选中的 flash id，`id < HAL_FLASH_ID_NUM` 与 `ret == HAL_NORFLASH_OK` 两条内层断言
都没触发。**这一轮买下的正是"第三颗 NOR 的 JEDEC 查表"那一格**，链路推进一层。

### 8.33 第 22 轮：新堵点定性 = `subsys_check_boot_struct` 读不到 sensor 核镜像的引导结构

**静态（Ghidra，file 0x8A8DFC / 0x8A7150）**：0x59 那一支的判据是
`func_0x008a7150(0x22000000, &buf) != 0`。`0x8A7150` 的名字从它自己的字面量池读出：
**`subsys_check_boot_struct`**。同一池解出的格式串与常量（file 0x8A71D0..0x8A71F4）：

```
'subsys_check_boot_struct'  '%s: addr not in flash: 0x%08X'  'id < HAL_FLASH_ID_NUM'
'%s: Failed to read flash: %d'  'ret == HAL_NORFLASH_OK'
'%s: boot_addr=%p magic=0x%08X'  期望常量 @file 0x8A71EC = 0xBE57EC1C
'%s: code_start=0x%08X'
```

函数形状（逐字读反汇编）：① 按地址取 flash id，`>=3` 断言行 0x1B5；②
`0x7FED88(id, boot_addr, &magic, 4)` 读 4 字节，非 0 断言行 0x1B7；③ 打
`boot_addr/magic` 日志后与 **0xBE57EC1C** 比，不等 `return 1`；④ 相等则
`*code_start = boot_addr + 0x10 | 1`（Thumb 位）返回 0。

**动态（`bootstruct.log`，三个站点串起来）**：

| 第 N 条 | 站点 | 读数 |
|---|---|---|
| 1,860,801,635 | `0x0C967176` | r0=2 r4=0x22000000（选中 id=2，boot_addr=0x22000000） |
| 1,860,803,364 | `0x0C967180` | **r0=0** ⇒ 读操作返回 OK，所以 0x1B7 那条没中 |
| 1,860,864,311 | `0x0C9671AC` | **r2=0x00000000（读回来的魔数）r3=0xBE57EC1C（期望）** ⇒ 不等 ⇒ return 1 ⇒ 断言 0x59 |

形态是"命令发了、状态报了 OK、数据全 0"。

**内容不是猜的**：`fw/p65_100_043/vela_sensor.bin` 头 16 字节现读
（`xxd -l 32`）= `BE57EC1C 00050000 00000000 2216E538`，第一字**逐字等于**固件那个比较常量；
`ota.sh` 里 `dd if=/ota/vela_sensor.bin of=/dev/sensor bs=32768 verify` —— sensor 核镜像
是一片独立的存储。等级：已验证（镜像自证 + 与固件常量同值），不需要再靠反汇编推。

**模型侧根因（读代码定案，不是推测）**：`fl_flash_init` 只给 flash2 接了寄存器形状，
**没有器件端点**。`src/l2/dev/flash.c:253-261` 的 RX_DATA 取字节只有两条来源——
挂了 `f->dev` 才走器件应答，否则只有 JEDEC 真值、其余一律回 0；而 NOR 阵列 `f->nor`
**只服务擦除/编程，不参与读**。所以"接上控制器"能过 JEDEC，过不了内容读。

**顺带更正上一轮的定性**：`sensor_ram`（0x22000000）那条注把它写成"sensor 核共享 RAM 窗，
装配期预置 4 字节魔数充当对端已就绪"。实测这份预置**一次都没被读到**
（`--watchr 22000000 1` 整轮零命中），固件是拿 `0x22000000` 当 **id=2 那片 flash 的窗口基址**
经控制器命令去读（这个地址本来就取自 file 0x37F8 那张按 id 索引的基址表）。
预置方向要从"往总线里塞 4 字节"改成"给那片 flash 挂上镜像内容"。

**工具教训**：上一轮我看设备窗口热点表（按访问次数截断的前 40 行）就下了"flash2 没有
FIFO 流量 ⇒ 没走命令口"，**是错的**：完整表里 `5016000C 读160`、`50160010 读44`、
`50160008 写7` 都在。低频格子被截断掉了。已给 flash2 补一份 `fl_flash_dump` 现场
（操作码分布、命令流水、收了几字节），以后这三颗控制器一律看各自的现场报告，不看热点表。

### 8.34 第 22 轮续：读命令的应答判据补上——第三片 NOR 挂上真内容

**取证三条**

1. **flash2 控制器现场**（本轮新补的 `fl_flash_dump`，`flash2_dump.log`）：
   ```
   命令寄存器=0000000B（操作码=0B 列=000000 要收=0）发送 FIFO 现有 1 字节 rx_index=1 应答类=无
   操作码分布：9F×12 05 35 06 01 77 AB FF 66 99 E9 0B×1
   FIFO 状态轮询 160｜收 44 字节(空应答 4)
   ```
   那笔 4 字节读用的是 **0x0B（fast read）**，命令字里**不带长度**、地址列 0，
   驱动按自己要的字节数逐次取 RX_DATA。
   重要: 上一轮"flash2 没有 FIFO 流量"的结论是**被热点表按次数截断骗了**——完整表里
   `+0xC 读 160`、`+0x10 读 44`、`+0x8 写 7` 都在。低频格子必须看设备自己的现场报告。
2. **跳板解码**（这条以后每次都会用到）：`0x7FED88` / `0x7FDE88` 都不是函数，是
   `ldr.w pc,[pc]`（半字 `F85F F000`）加一个指针字；指针分别 = `0x003670FD` / `0x003672E1`，
   落在 SRAM 执行别名段 ⇒ 影子源 **file 0x37215 / 0x377F9**（别名 − 0x363AE8）。
   根因: 这类驱动**在倒灌代码段里**，不在主代码段。Ghidra 只给出 `func_0x007fxxxx()` 一跳板时，
   先读它后面那个字，别在主代码段里找它。
3. **内容口径**：`0x22000000` 是第三颗 NOR 的**窗口基址**（整片偏移 0 在地址空间里的位置），
   内容 = `fw/p65_100_043/vela_sensor.bin`；头字 BE57EC1C 与固件比较常量（file 0x8A71EC）同值，
   `ota.sh` 的 `dd if=/ota/vela_sensor.bin of=/dev/sensor` 自证这是独立的一片存储。

**模型改动（四处，全部只影响被点名的这一颗）**

- `src/l2/dev/flash.h`：应答类新增 `FL_FLASH_RESP_ARRAY`；结构体加**实例级**清单
  `array_read_op[] / n_array_read`（清单为空 ⇒ 行为与改动前逐字相同）。
- `src/l2/dev/flash.c`：`respond_kind()` 在"没挂器件端点、挂了阵列、操作码被点名"时答 ARRAY；
  RX_DATA 在 ARRAY 这一支**不看命令字里的长度**，按"整片偏移 + 已取序号"从阵列宿主指针给字节，
  越界计入 `n_drop`（不静默）。
  注意这里**没有动 FIFO_STATUS**：实测驱动不等应答计数（整轮 +0xC 只轮询 160 次，
  没有 8192 那种等待超时的形态），所以不臆造"应答剩余字节数"——没有证据就不改。
- profile（`best1502_r0.soc.json` + `flint_profile.py`）：`sensor_ram` 更名 **`sensor_flash`**
  （region 0x22000000+0x170000，`init_fill 0xFF` 按擦除态铺），`flash2_ctrl` 加
  `nor_region / nor_window / array_read_opcodes=[0x0B]`；`array_read_op` 刻意挂在**实例**上
  而不是 inherits 父表——走 inherits 会一次点到三颗，把 flash_ctrl（内容本来 XIP 直读）
  与 flash1_ctrl（挂 NAND 器件端点）的应答判据一起改掉。
  校验器同步加了一条门：sensor 镜像的 sha256/字节数/装不装得进窗口，以及
  "fw 声明了镜像但 SoC 没有承接窗口"。
- `tests/unit/test_boot.c`：装配期把镜像**整片铺进窗口并校验头字**（不等就 `CK` 大声失败），
  替换原来"只写 4 字节魔数"的伪应答；给 flash2 挂上 `g_nor2` 阵列并报名字（含点名清单）；
  顺手给这颗补了 `fl_flash_dump` 现场。

**撤销的定性（第三处命名先行）**：`sensor_ram`/"核间共享 RAM 窗"不成立——那次读走的是
flash 控制器命令而不是总线 load（`--watchr 22000000` 整轮零命中）。窗口现在按它的真实身份命名，
`magic` 那一格也从"预置值"改成"载入后的校验门"。

**全量构建**：`bash flint-vm/build.sh` 通过（12 单测 + 20 组对照组 + 两个 CI 门 + 三份 profile，
p65 profile 666 行 / 57 项跳过）。

**判据（`sensor_nor_run.log`）**：① 站点 `0x0C9671AC` 的 r2 是否变成 BE57EC1C（与 r3 同值）；
② 断言哨兵 `0x0C4F74E8` 是否不再命中行 0x59（若命中 0x8B/0x8E 说明又推进一层）；
③ rc 执行器 `0x0C5B47E0` 是否命中；④ UART 是否出 `kvdbd`；⑤ lcdc 帧是否 >0。

### 8.35 第 22 轮续二：阵列在答，但差一个 dummy 槽位（0x0B 有 8 个等待周期）

`sensor_nor_run.log` 的三条读数：

| 第 N 条 | 读数 |
|---|---|
| 1,860,864,651（站点 `0x0C9671AC`） | r2=**0x00BE57EC**、r3=0xBE57EC1C —— 还是不等，但已经不是 0 了 |
| flash2 现场 | `收 44 字节(空应答 0)`（改之前是 `空应答 4`）、`应答类=阵列字节`、`rx_index=5` |

`空应答 0` 说明阵列这一支通了；`rx_index=5` 说明这笔"4 字节读"**弹了 5 个字节**。
镜像头 5 字节是 `1C EC 57 BE 00`，丢掉首字节后用后 4 个按小端拼字 = `EC 57 BE 00` =
**0x00BE57EC** —— 与站点读数逐字节吻合。
根因: 0x0B（Fast Read）带 **8 个 dummy cycle**，驱动先弹一个等待周期的空槽再取数据；
而模型把 dummy 那一槽也当成阵列字节发了出去，整串数据因此右移一字节。

**改动**：`array_read_op` 现在每条读操作码带一个 dummy 槽位数（profile 里
`{"opcode":"0x0B","dummy_bytes":1}`），模型里前 N 个应答槽位**回 0 且不推进阵列指针**，
第 N+1 个起才从 `整片偏移 + (序号 - N)` 给字节。`respond_kind()` 与 RX 两支都改走
`array_read_index()` 一份查表，免得两处判据漂移。

**新增单测 [F9]**（`test_flash.c`，不依赖 p65 目标）：把点名与 dummy 槽位直接摆好后验四件事
—— ① 操作码被认成"按阵列答"；② dummy 那一槽回 0；③ 后 4 槽按小端拼出的字等于阵列里那 4 字节；
④ `rx_index` 停在 5、`n_drop` 为 0。再加一支"清掉点名清单后同一条命令回到无应答、
`rx_short` 记账 +1"，证明判据真的跟着清单走。
`--sabotage` 对照组照旧要求失败，已成立。**全量 `build.sh` 通过。**

**下一格的静态取证已备好**（`dummy_run` 一到就能对上）：`0x8B2D64` 的字面量池自报名字
**`hal_rmt_ipc_open`**，池里另有 `'RMT_IPC-OPEN: Failed to get cfg: core=%d'`（断言行 0x167）、
`'cfg->chan_cfg[chan].chan_manual_rx_done == manual_rx_done'`、
`'Failed to alloc wake lock idx'`。它的取配置函数 `0x8AB288(core)` 按核号返回
**SRAM/PSRAM 里的运行期对象指针**（core2 → 0x20195D50、core1 → 0x3C3CECA4），
然后逐字段判非空 —— 所以这一格能不能过，取决于那张表有没有在更早的初始化里被填上；
`0x0C972D90` 起那一串 `ldr/ cmp #0 / beq` 就是逐字段的空位检查。

**判据（`dummy_run.log`）**：① `0x0C9671AC` 的 r2 是否 = BE57EC1C；② 断言哨兵是否不再命中 0x59
（改中 0x8B/0x8E/0x167 就是又推进一层）；③ rc 执行器 `0x0C5B47E0` 是否命中；
④ UART 是否出 `kvdbd`；⑤ lcdc 帧是否 >0。

### 8.36 第 22 轮收束：引导结构闸**过了**——断言链第一次整段消失

`dummy_run.log`（2.4G 上限，但没跑到）：

- 站点 `0x0C9671AC`（第 1,860,864,780 条）：**r2=BE57EC1C、r3=BE57EC1C** —— 读回的魔数
  与固件比较常量**同值**，`subsys_check_boot_struct` 返回 0。
- **断言哨兵 `0x0C4F74E8` 一次都没命中**（这条链从头到尾没有 assert）；
  rc 执行器 `0x0C5B47E0` 也还没到。
- 停机原因换成全新的：**第 1,860,932,747 条，总线错误，PC=0x0C973056**，
  编码 `0000601C` = `str r4,[r3,#0]`（不是 `[pc]` 那一族：imm5=0、Rn=3、Rt=4）。
  坏访问 **addr=0x20320000 类型=unmapped**，整轮 `未映射 1`（就这一次）。
- 现场寄存器：r3=0x20320000、**r4=0x20195D50**（就是 `hal_rmt_ipc` 的核 2 配置对象）、
  BASEPRI=0、SP=PSP=0x3C5D0AE0（还在任务栈里，异常计数与上一轮同量级）。

**这一格的代码形态（Ghidra，file 0x8B3034-0x8B3056）**：

```
008B303A  ldr r3,[r4,#0x44]     ; cfg->chan_cfg[...]（通道表）
008B304C  ldr r1,[0x008b3150]
008B3050  str.w r2,[r1,r3,lsl #2]
008B3054  ldr r3,[r4,#0x8]      ; r3 = cfg->+8 = 0x20320000  <- 固件自己带进来的指针
008B3056  str r4,[r3,#0x0]      ; *cfg->+8 = cfg（把配置对象挂进那块窗口）
```

两点结论：
1. **`cfg` 对象是有内容的** —— `cfg->+8` 是个像样的指针 0x20320000，不是 0。
   这说明它由 `.data` 段的初值搬进 SRAM 的，我们此前"crt0 的 .data 搬运可能没生效"
   的怀疑在这一格上不成立（至少这几个字段是对的）。
2. **堵点性质从"固件判据失败"换成"我们的地址图缺一段内存"**。
   0x20320000 高于 MCU RAM 顶（crt0 写 MSP=0x2019FFE0 ⇒ MCU 那段到 0x201A0000；
   best1503 的权威口径也才 0x20000000-0x20200000），所以它是**另一段**——
   核 2（sensor）那侧的共享 RAM，AP 直接把配置对象挂进去。
   profile 里现在没有任何区间覆盖它（`sensor_flash` 是 0x22000000 那段，另一回事）。

**下一步（取证已在跑，`rmtcfg_run.log`）**：把 `cfg` 对象整体读出来
（`--site 0C973054 --site-mem r4` 取 48 字节 + `--ramdump 20195D50 128`），
按字段列出 AP 会碰的**所有**对端地址，再据此建区间 —— 尺寸与归属要有出处，
拿不到出处就按"覆盖实测被碰到的最高地址"建并标 推断待核（这条与 §8.31 的
命名教训同源：**先拿到被访问的地址集合，再决定窗口**，不许先起名）。

### 8.37 第 22 轮续：cfg 对象整体读出来——被碰的对端地址只有两格

`rmtcfg_run.log`：站点 `0x0C973054`（第 1,860,932,746 条）带 `--site-mem r4` +
`--ramdump 20195D50 128`，把 `hal_rmt_ipc` 的核 2 配置对象读成一份可解析的字节。逐字段：

```
+00  3C2E3745  -> "MCU2SENS"                    链路的自报名字（三条链路名里的那条 sensor）
+04  20320004                                    对端窗口的第二格
+08  20320000                                    对端窗口的第一格：*这里 = cfg 自己
+0C  0C972A29 / +10 0C972A5F / +14 0C972A73 / +18 0C972A87 / +1C 0C972A9F
+20  0C972B09 / +24 0C972AB7 / +28 0C972AFD / +2C 0C972AF1 / +30 0C972AE5   11 个函数指针(ops 表)
+34  00030002                                    两个 16 位字段（半字 2 与 3）
+38  2019E4D0  +3C 2019E530  +4C/50 2019E728      全在**已建的 MCU SRAM** 里（<=0x201A0000）
+40  3C2E374E -> "OPST%s-RX:%d: Got peer=%p (%p)"
+44  3C2E3750 -> "ST%s-RX:%d: Got peer=%p (%p)"
+48  00000001      +54 之后全 0
```

三条判据性结论：
1. **对象是有内容的**（`+0` 是名字、`+C..+30` 是 vtable、`+38/+3C/+4C` 是 MCU 侧通道结构）
   —— 前面"crt0 的 .data 搬运可能没生效"这条怀疑在这一格上**不成立**，
   `hal_rmt_ipc_open` 那串"逐字段判非空"的闸是因为字段真的有值才过的。
2. **越界点只有两格**：0x20320000 与 0x20320004（`cfg->+8/+4`）。整轮 `未映射 1`，
   就是这个 `str r4,[r3]`。其余地址都落在已有区间里。
3. `+40/+44` 那两条格式串说明 open 之后还要**等对端把 peer 指针挂上来**
   （"Got peer=%p"），也就是说下一格大概是"等核 2 应答"，不是又一次空指针崩溃。

**改动**：`best1502_r0.soc.json` 新增区间 `rmt_ipc_peer` = **0x20200000 + 0x200000**（ram，初值 0）。
- 依据：上面那两格被碰的地址；上界取到 0x20400000 是因为 0x201A0000(MCU RAM 顶) 之上
  到 0x22000000(第三颗 NOR 窗) 之间 profile 里什么都没有，把实测两格连同余量包住。
- 诚实标注：**归属与容量都没有权威源**（SDK 头、参考实现里都搜不到 0x2032xxxx），
  等级=推断待核；初值刻意保持 0 = "对端还没登记"，不预置假 peer —— 真机语义是
  谁先跑谁先把自己的 cfg 挂进这两格。
- 只进 p65 的 profile：`profile.prf`（10 Pro p67 那条线）里没有这段，构建门确认过。
  全量 `bash flint-vm/build.sh` 通过。

**判据（`peer_run.log`）**：① 还停不停在 unmapped（应换成"推进到更晚"或"等应答"形态）；
② 断言哨兵 `0x0C4F74E8` 是否仍零命中；③ rc 执行器 `0x0C5B47E0` 是否终于命中；
④ UART 是否出 `kvdbd`；⑤ lcdc 帧是否 >0。

### 8.38 第 22 轮收束（重要）：对端窗口补上之后，引导第一次走过 1.86G——rcS 在跑、kvdbd 已生成

`peer_run.log`（2.4G 上限，被 `--uart-stop kvdbd` 提前叫停在 2.067G）：

| 信号 | 读数 |
|---|---|
| rc 执行器站点 `0x0C5B47E0` | **命中**，第 1,962,974,755 条（r0=3C5C12F0、r1=3、lr=0x0C5AE34B）—— 引导走到 NSH 的命令执行 |
| `--uart-stop kvdbd` | **命中**，第 2,066,895,888 条（PC=0x0C4D75B8）—— rcS 已经在创建守护进程 |
| UART1 字节数 | **24,782**（此前十几轮一直停在 2,850-2,926） |
| 断言哨兵 `0x0C4F74E8` | 零命中 |
| 总线 | `未映射 0`；非对齐 15,142（就是 0x201969B8 那处既有形态） |
| 异常 | 进入 1,952,335 / 返回 1,952,334（停机那一瞬在 BASEPRI=0x80 的临界区里） |

意义：**"卡在 1.86G"这条持续了十几轮的现象结束了**。sensor 核那一段（第三颗 NOR →
镜像引导结构 → hal_rmt_ipc_open → 对端窗口）整条链在模拟里通了，而且没有靠任何伪造预置 ——
两处旧预置（`sensor_port` 的 FIFO_STATUS、`sensor_ram` 的 4 字节魔数）都已撤销，
换成"真的控制器 + 真的镜像内容 + 真的对端内存窗口"。

下一判据是 **lcdc 帧 > 0**（`--panel` 只有真出帧才写 PNG，观感判据从这里来）。
已开 8G 长跑（`long_run.log`，`--beat 250000000`，带 `--panel build/p65_scan/p65_frame.png`）。

**顺手沉淀：`0x7Fxxxx` 那批 veneer 的解法**（这条链上每个都用到）
半字 `F85F F000` = `ldr.w pc,[pc]`，紧跟的那个字就是真目标（多为 0x0036xxxx 执行别名），
**影子源 file = 别名 − 0x363AE8**。本链已解出的：

| veneer | 真目标别名 | 影子源 file |
|---|---|---|
| `0x7FBF48`（ID 查表） | 0x00366E71 | 0x3388 |
| `0x7FDE88`（按地址取 flash id） | 0x003672E1 | 0x377F9 |
| `0x7FED88`（读阵列字节） | 0x003670FD | 0x37215 |
| `0x7FEA90` | 0x003671BF | 0x36D6 |
| `0x7FC148` | 0x00369F3D | 0x6454 |
| `0x7FD528` | 0x00369949 | 0x5E60 |
| `0x7FBD70`（open 末尾那步） | 0x00369995 | 0x5EAC |
| `0x7FEE58`（日志转发） | 0x0036F42D | 0xB944 |

这批全落在 **file 0x3000..0xC000 的倒灌代码段**里 —— 主代码段找不到它们是正常的，
别再去主段找。

### 8.39 第 22 轮续：8G 长跑把堵点换成了文件系统——yaffs 分配器耗尽

`long_run.log`（--deep 8G，跑满没停机）到 8G 的读数：`异常 进入 16,141,966 / 返回 16,141,965`、
`UART1 写出 101,971 字节`、`未映射 0`、**lcdc 帧仍 0**、TE 给了 126 次。
尾部 8KB 日志里刷的是同一个循环：

```
yaffs: yaffs tragedy: no more erased blocks
yaffs: !!!!!!!!! Allocator out !!!!!!!!!
yaffs: **>> Block 24 retired
yaffs: Block 24 is in state 9 after gc, should be erased
... (25/26/27/28/29/30/31/32 依次)
```

**先把这一段的判据顺序钉住**：引导脚本是资源分区 ROMFS 里的 rc.sysinit，
从 `fw/p65_100_043/vela_resource.bin` file 0x3018C10 现读全文（13 行）：

```
set +e
mount -t procfs "/proc"
mount -t tmpfs /tmp
pmconfig stay normal
mount -t romfs /dev/resource /resource
mount -t yaffs -o autoformat /dev/nand_data /data
offline_log start &
rptun start /dev/rptun/sensor
rptun start /dev/rptun/audio
mount -t rpmsgfs -o cpu=sensor,fs=/proc /mnt/sensor
kvdb_init
kvdbd &
chargerd &
```

三条结论：
1. `/data` 是 **yaffs on `/dev/nand_data`**（`-o autoformat`），不是 littlefs、也不是 NOR 上那个
   `storage.nor` 段。所以 §8.38 之后新的第一障碍是**存储层**，与 sensor 核无关。
2. `kvdbd` 在脚本第 12 行 —— 上一轮 `--uart-stop kvdbd` 命中说明 **rc.sysinit 已经走到倒数第二行**，
   yaffs 挂载与 `rptun start sensor` 都返回了。这一条把"卡点=引导"改写成"卡点=运行期写 /data"。
3. yaffs 报的是"擦除态不够用 + 每块 gc 之后不是 erased"，形态像**几何/擦除口径对不上**
   （擦过的块再读会碰到旧字节），而不是固件的分配策略问题。

**分区表线索（等级=推断待核，等日志里的几何打印来对）**：AP 镜像 file 0x2E4D00 与 0x2E4D70
两处名字池里都有 `nand_data`，旁边挂着三个数：`0xDC00`、`0x1B800`、`0x24800`。
若按"页号"读：0x1B800=112,640 起、0x24800=149,504 止 ⇒ 36,864 页 = 576 块（64 页/块）≈ 72MB；
resource 那份 romfs 占 106,670 页，正好落在 112,640 之前，形态自洽。但这三个数的单位没有出处。

**下一步**：已经给 test_boot 加 `--uart-log <口> <文件>`（整段 102KB 落盘，尾部 8KB 看不见
第一次失败），正在跑 8G 并抓全量日志 —— 要看的是挂载那几步的原始输出：
yaffs/NuttX 打出的 geometry（块数、页大小、OOB、坏块标记位置）与**第一次 retired 之前那几行**。

### 8.40 第 22 轮续：全量日志到手——yaffs 不是"没块"，是**每一页写进去都验不过**

`--uart-log uart1` 落下的完整固件日志：`build/p65_scan/p65_uart1.log`（101,971 字节 / 2,597 行）。

**时间线（按日志行号）**

| 行 | 内容 | 读法 |
|---|---|---|
| 52 | `yaffs: 0 blocks to be sorted...` | 挂载扫描一开始就"无块可排"，说明扫不到任何有效 chunk |
| 55..2396 | `>> Block 1 needs retiring` 一直排到 **Block 2336** | **每一个块**都被判要退休 |
| 1281-1282 | `nsh: losetup: ioctl failed: 2` / `nsh: mount: mount failed: 15` | 脚本第 10 行 `mount -t rpmsgfs -o cpu=sensor` 那一条 —— 对端核不在，预期失败，`set +e` 不挡后面 |
| 2397-2399 | `no more erased blocks` / `Allocator out` / `>> yaffs write required 2336 attempts` | 试遍了 2336 个块，没有一个写得进去 |
| 2400+ | `Block N is in state 9 after gc, should be erased` | gc 之后那块还不是擦除态 |
| 2404/2413/2414/2427 | `miwear_bluetooth [26:103]`、`nfc_stack_bridge [31:200]`、`miconnect [35:100]`、`miwear [50:104]` | **应用任务确实在建**；全程 0 处 screen/lvgl/watchface 输出 |

**两条硬证据（几何）**
- 2336 块 × 64 页/块 = **149,504 页 = 0x24800** —— 与镜像分区名池里 `nand_data` 旁边那个数（file 0x2E4D78 一带的 `0x24800`）逐字对上。
  ⇒ `/dev/nand_data` 的跨度是 **页 0..0x24800**。
  重要: 这**推翻**了 profile 里 `storage.resource.first_page=0 / page_count=106670` 那份"照抄 p67 待核"的口径 ——
  照那份，romfs 铺在页 0..106,669，正好压在 yaffs 的设备范围里。这条要么改成"两片 MTD 各有自己的基址"，
  要么 romfs 根本不在同一个 0 起点上。**这一格必须先把两片分区的地址口径核清楚再动模型**。
- 编程命令的形状（flash1 现场，2448 次）：`06 → 00080032（0x32 装载，列=0x800=2048）→ 01C58010（0x10 执行，row=0x1C580）`。
  列给的是 **0x800（备用区起点）**，不是 p67 那条"列 0 一次推 4 页主区"。

**为什么每一页都验不过（模型侧判据链）**
- GPDMA 的 flash **写方向 0 次 / 0 字节**（读方向 13431 次 / 27,506,688 字节正常）；
- 发送 FIFO 总共只写了 64,899 字节，摊到 2448 次编程 ≈ **26 字节/次** —— 一页 2048 字节的主区**不可能**走这条；
- 而模型的编程数据取自"命令 TX FIFO + prog_cache"，`prog_cache` 每次 0x32 都先 memset 成 0xFF，
  执行时按 **AND** 落进页 ⇒ 主区被 0xFF 盖过去 = **写进去的全是擦除态**；
- 驱动验页失败 ⇒ yaffs 判块坏 ⇒ 2336 块全 retire ⇒ Allocator out。

结论：**这不是 yaffs 的策略问题，是 p65 的 NAND 数据面没接到模型里**。p65 与 p67 在这一点上不同形
（p67 实测"列 0 推 4 页主区"，p65 一次都没触发那条），所以照搬 p67 的写路径必然出现这种"写得进、读不回"。

**正在跑的取证（`wstop_cache.log`）**：`--wstop 30000000 2112 1860000000` 抓"谁在写映射缓存窗口"。
假设 A：驱动把主区**内存直写**进 0x30000000 那个 64KB 窗口（窗口就是器件的 page cache），
再发 `32(列)/10(row)` 提交 —— 那模型的编程就该**从窗口取字节**，`prog_cache` 那条是多余的。
假设 B：还有第三条数据面（另一个 DMA 通道/另一个口）。
拿到写者的 PC 之后直接用 Ghidra 读那段驱动定案，不许靠猜改模型。

### 8.41 第 22 轮续二：yaffs 全块 retire 的根因**定案**——模型在装载那一刻取样，页数据是之后才推进 FIFO 的

三条实测（同一格 flash1 控制器，`--wtrace 40500000 20 1500 1870000000` 抓到 1500 笔写）：

1. **写侧次序**（一段完整编程周期的形状）：
   ```
   推 64 字节进 +08 → +00 <- 0x00000032（装载，列=0） → 再连推 261 笔（宽4）进 +08
   → +04 <- 0 → +00 <- 01C14010（执行，行=0x1C140） → +18 <- 3（提交）
   ```
   偏移分布 `+08:1309 / +04:105 / +00:40 / +18:30`，`+08` 那 1285 笔的 PC 全是
   `0x00367430`（影子源 file 0x3948，就是 FIFO 推字循环）。
   收尾那一刻：**命令寄存器=00000032、发送 FIFO 现有 1556 字节**。
2. **FIFO 容量 2112** = 一页 raw（2048+64），与参考实现 `MiBandFlash.tx_fifo` 同容量 ——
   设计意图就是"整页从发送 FIFO 走"。
3. **模型侧的行为**：`fl_nand_command()` 只在**装载命令那一刻**读一次 FIFO；
   那一刻里面只有上一段残留的 64 字节（`stage_hist` 1946 次全是"≤64"、`stage_max=64`），
   之后推进来的整页没人接 —— 提交那一路 `commit_tx` 只认特征字节就把 FIFO 清了。
   于是每页落盘都是擦除态，驱动回读校验必然不过，
   yaffs 把 2336 个块逐个 retire 到 `Allocator out`。

**改动（`src/l2/dev/nand.c`）**：执行命令（0x10）那一刻先把"装载之后新增的那一段 FIFO"
接进写缓存（`stage_seen` 记住装载时已经收过的前缀，避免重复落），再接着走原来的落盘路径；
装不进写缓存的次数单独计 `stage_drop` 并打进 dump。装载分支的取样保留
（p67 那一版是"数据先到、命令后发"，两条次序都成立）。
**口径上的诚实**：驱动到底把 spare 放在页内哪一段（实测一段是 64 字节 + 261 个字），
这份改动只保证"写进去的字节原样读得回来"（yaffs 自洽），
**不等于 spare 布局与真机逐字节一致** —— 那一条要等能读回真机 yaffs 镜像或 NV 内容时再核。

**顺带修的两件探针口径（都是"假零命中"的来源）**
- `--wstop/--rstop` 现在除了比原始地址，还比"物理对象 + 区内偏移"：同一份器件窗口
  有两个别名（nand_cache 的 0x30000000 与 0x34000000 是同一份字节），只比原始地址时
  我差点把"驱动不写缓存窗口"当成结论写下去 —— 那本来会是假阴性；补成双口径后
  确实仍零命中，才敢判"驱动不走缓存窗口写"。
- `--wtrace/--rtrace` 加了第 4 个参数（起拍）。晚段（运行期写盘）与初始化段的命令
  次序不是一套形状，从头打会被前一两千条占满配额，看不到要看的周期。

**新增统计**：`col_hist`（装载命令给的列号分桶）、`stage_hist`（执行那一刻已装字节分桶）、
`stage_max`、`stage_drop` —— 就是这些数把"数据在哪条路上"从猜测变成了读数。

### 8.42 显示链锚点清单（并行静态取证，本会话复核了关键几条）

`/data` 通了之后要让 UI 起来，事先把 p65 的点屏链路钉成锚点表（地址都进
`docs/fw_address_table_p65.csv`）。口径自证：同一次 headless 里 `fn 0x890818` 有结果、
`fn 0xc950818` 报"不在任何函数里" ⇒ Ghidra 空间是 file offset；
函数指针要 `|1` 的实证是 file 0x423400 表项值 `0x0C4E318D` 指向入口 `0x42318C`。

两条**并行**的路（静态没有直接 bl 衔接，这一条别搞混）：

```
A 点屏会话路  screen_turn_on 入口 file 0x9A9200 → 本体 file 0x9A7CF0
              本体只登记状态 + 起定时器（不碰驱动）
              前置门：会话计数 *(0x3C3CA330+0x1BC)、FUN_009A2304 返回值、FUN_0086B278
B 出帧路      帧请求 file 0x8A89CC（run 0x0C9689CD）
              BASEPRI 临界区里判 *(u8*)(0x3C3CA330+0x150) < 2
              → FUN_007FEF50 / FUN_008A84B0(3,1) → +0x154++ → +0x148 = 1
              另一入口是 ioctl 0xFB（FUN_0044F63C @ file 0x44F72A）
```

面板身份那条链（**第一帧之前必须过的**）：
- 识别：`FUN_0041E4DC` 用 `bl 0x8A3132(0、4、buf、3)` 循环 10 次发 DSI 读命令取型号字，
  失败就走掉电重上电；调用者 file 0x452D0A。
- 落点：**DSI 读命令的应答写进 PSRAM 0x3C3D4F10**（不是 SRAM 预置、也不是 I2C）；
  相邻 0x3C3D4F14 存识别结果指针、+4 是成功位。
- 派发：`FUN_00423300` 比 `0x00200135 / 0x00200145`（rm690c0-VSN）与
  `0x00200245 / 0x00200235`（TM），末句 `bl 0x41F0C4(0x0C4E318D、0x3C3D54F8)`。
- 面板驱动：`FUN_0042318C` = rm690c0 的"上电 + 显示开"，池里有
  `'rm690c0 display on,0A reg is 0x%x'` 与 GPIO 值 `0x14040/0x14042/0x1402D`。
- 背光门：面板结构 `0x3C3D54F8 + 0x411 = 0x3C3D5909`，`FUN_0041F600` 要求 `(v-2)<2`
  才写真背光寄存器。

四个**缺口**（都不许当已知用）：G1 `show logo` 的调用者（无表项无引用）；
G2 谁写 `0x3C3CA330+0x150`（要动态 `--wstop`）；G3 A 路怎么衔接到 B 路；
G4 谁把 `+0x411` 置成 2/3。

**自我纠正一条**：这轮定位时先把 `FUN_009A7488` 与 `FUN_009A7E10` 认成了
`screen_turn_on`，两条都错 —— 它们其实是 `turn_off_aod` 与 `apply_aod_settings`。
根因是同一个 `__func__` 串被 8 个函数的字面量池共用，**只能靠"池格 xref + 同池的
trace 格式串"定归属**，光看池里有没有那个名字会认错。这条与 §8.31 的教训同一类。

### 8.43 第 22 轮验证：NAND 数据面接上之后 `/data` 干净挂载，引导日志从 99KB 报错降到 3KB

判据是**同一份日志的两份对比**（`--uart-log` 落的原始字节，不是读数）：

| 计数 | 改之前 `p65_uart1.log` | 改之后 `p65_uart1_v2.log` |
|---|---|---|
| 固件日志总量 | 99,427 字节（几乎全是 yaffs 报错） | **3,185 字节** |
| `needs retiring` | 2336 | **0** |
| `yaffs tragedy` / `Allocator out` | 40 / 40 | **0 / 0** |
| `block N is in state 9 after gc` | 56 | **0** |

NAND 侧自己的账也对上了：`stage_hist` 从"1946 次全是 ≤64"变成"14 次全在 ≤2112 那一桶、
`stage_max=2048`" ⇒ 整页主区**真的进了写缓存**；`编程载入 14 / 执行 14 / 擦除 0`
是因为不再需要拿 2336 个块去试错。

**改后引导日志的实际内容**（尾部原样）：
```
yaffs: 0 blocks to be sorted...
offline_log [9:100]      log_service [11:100]     kvdbd [13:101]
miwear_rtc_checker [14:100]      chargerd [15:100]        bluetoothd [19:100]
nsh: losetup: ioctl failed: 2    nsh: mount: mount failed: 15
miwear_bluetooth [25:103]        nfc_stack_bridge [31:200]  miconnect [35:100]
[func] softbus_eng_init          [func] networking_eng_init
```
⇒ rc.sysinit 十三行里，前 9 行全部成功；两条失败正是脚本第 10 行
`mount -t rpmsgfs -o cpu=sensor,fs=/proc /mnt/sensor`（对端 sensor 核不在，`set +e` 之后继续），
它**没有挡住**后面的 kvdbd / chargerd / miwear 系任务创建。
所以 §8.38 记的"下一格是等核 2 应答"要修正一句：sensor 核不在只挡住 rpmsgfs 那一条挂载，
不挡 UI 线程的建立 —— 核 2 的对端应答器因此**降级为后面的可选项**。

**当前追的东西**：8G 长跑（`ui8g_run.log`），站点挂在
`0x0CA69200`（screen_turn_on 入口）、`0x0C9689CC`（帧请求）、`0x0C4F74E8`（断言哨兵），
判据仍是 `lcdc 帧>0`（真出帧才写 PNG）。这一段日志里已经**没有任何存储层报错**，
所以再往下碰到的应当就是显示/输入/表盘那几条链。

### 8.46 第 23 轮：推翻上一条判据——固件在 5.5G 就已经停机，"系统在干活"那句作废

**先写纠正**（纪律 18）：§8.43 结尾我根据"热 PC 榜有 947 个不同地址"判"系统在干活、不是在自旋"。
**这句是错的**。地址多是因为前 5.5G 真的在跑；5.5G 之后整机已经进关中断死等。

8G 长跑（`build/p65_scan/display_run.log`）四个连续 beat 的现场一模一样：

```
[beat] 5600000000  PC=0036EB36 LR=0036E5A7 SP=3C5BF2B0｜异常 进=9419422 返=9419422 pend=1242
[beat] 6400000000  PC=0036EB36 LR=0036E5A7 SP=3C5BF2B0｜异常 进=9419422 返=9419422 pend=1243
[beat] 8000000000  PC=0036EB36 LR=0036E5A7 SP=3C5BF2B0｜异常 进=9419422 返=9419422 pend=1243
```

`异常 进入 == 返回` 且**从 5.6G 到 8.0G 一条都不涨** = 中断不再被取，24 亿条指令全在同一条自环上。
热榜第 1 名 `0036EB36` 占 31.54%（2523/7999 采样），第 2~5 名 `0036FB94..9A` 各 1.5%（同一条 4 指令循环）。

**换算与语义**（别名 − 0x363AE8 → file）：`0x36EB36 → file 0xB04E`，落在 `FUN_0000b030` 的最后一条
`b 0x0000b04e`。Ghidra 反编译该函数体：

```c
func_0x00006558();      /* 刷新输出 */
FUN_0000c0a8(0x50);     /* 忙等 */
for (i = 10; i; --i) FUN_0000c0a8(0x10);
FUN_0000aa28();         /* 收尾动作 */
while (1);              /* 死等 */
```

——"刷串口 → 等发送排空 → 收尾 → 死等"，是 abort/复位那一类尾段，不是渲染路径。
唯一引用点是 file `0xAF20` 的一条 `beq.w 0x0000b030`；它前面那一段在做 AON 域寄存器的读改写
（`movw r0,#0x303f / 0x311e / 0x3117 / 0x3119 / 0x304f` 配 `bl 0x6c9c`/`bl 0x6d24`，掩码取低 15 位），
并按 `[r5]` 的字节标志依次调 `FUN_0000acbc(1|2|4)`。`FUN_0000acbc` 的入参 1/2/4 与 PM 唤醒源那一族吻合。

**判据因此换一个**：在追到第一帧之前，必须先回答"固件为什么在 5.5G 决定停机"。
日志尾部只到 `miwear [52:104]`，**没有任何 panic/assert 文本** —— 说明尾段的打印要么走的是别的口
（UART0 本轮 0 字节、UART2 0 字节），要么这条路径本来就不打印（硬件看门狗/复位类）。

### 8.47 第 23 轮：显示层现状定案——帧请求真的被调用过，缺的是"没人搬帧 + 我没做 vsync"

一次 8G 长跑把显示链的证据拿全了。

**帧请求命中两次**（站点 `0x0C9689CC` = file 0x8A89CC）：第 257,732,346 条与第 2,476,746,868 条，
lr 都是 `0x0C95055B`。调用者是 `FUN_00890470`（`bl 0x008a89cc` @file 0x890556）——
一个按 11 位宽高算 stride、带颜色格式分支（`[r7,#0x37]` 比 0x20 / 0x18）的渲染函数。
这顺带把 §8.42 的缺口 G1 补上了：`show logo → FUN_00890470 → 帧请求` 这条链**动态成立**。
另一个调用者是 ioctl 0xFB 那条 `FUN_0044f63c`（file 0x44F72A）。

**帧请求体内**（逐字读过）：`basepri=0x80` 的临界区里，若 `*(u8*)(obj+0x150) < 2` 则

1. `0x007fef50(0xb, 7)` —— veneer：`DAT_007fef54 = 0x0036B559` → 别名 − 0x363AE8 → file **0x7A70**，
   SRAM 核管理的 PM 资源申请（入口 `cmp r0,#0x2d` 是资源上界、`cmp r1,#0x7` 是状态上界，
   随后按资源号把状态写进一张字节表）。资源 0xb、状态 7。
2. `FUN_008a84b0(3, 1)` —— 把活动掩码字节 `*DAT_008a8500` 的 **bit3** 置 1；
   掩码由 0 变非 0 时调 `FUN_008adcf8(0,2)`、归零时调 `FUN_008add68(0,2)`（wakelock 那一类）。
3. `*(obj+0x154)++`、`*(obj+0x148) = 1`。

**LCDC 侧实测**（`lcdc 读39 写59 认不出0`）：31 个寄存器格被碰过，写 PC 全在 file 0x8B1Dxx–0x8B29xx
的一族小 setter（每个都是"读-改-写 + 位移"，基址从字面量 `DAT_008b28f4` 取 = 0x40200000）。
块配置 `+0x0F0/+0x0F4`、几何 `+0x0FC/+0x104/+0x108`、`+0x264` gra_config、
`+0x1C0` irq_enable = 0x00700000（bit20/21/22）都写了 ⇒ **控制器是完整配置起来的，不是没开**。
但是：

- `+0x224` frame_start **一次都没写**；`+0x280` scanout_en 也没写；
- `lcdc 帧=0 中断=0`、`gpdma 段=0 搬运=0`、`te 给=0 跳=192`；
- DSI 反倒活着：`dsi 应答包3 供字节20`、`+0x18` 命令格写 21 次 —— 面板命令通路是通的。

**我自己欠一项**（记账）：profile 里 `lcdc.frame_ns = 16666666` **没有任何 C 代码在用**。
现有模型只有"写 frame_start → commit → 置 done 位并 pend 中断"，没有自由运行的 vsync 源。
若固件的流水是"配置完就等 vsync/帧完成中断才启动传输"，那这条是**模型缺口**，不是固件没走到。
（注意先后：先解决 §8.46 的停机，再动 vsync，否则分不清是谁挡的。）

**一条阴性结论还没自证**：`--rtrace 3C3CA478 8 80 240000000`（盯 obj+0x148/+0x150 的读）
从 240M 跑到 3G **零命中**，字面意思是"帧请求置上的标志没有任何消费者"。
这是阴性判据，按纪律必须先拿一个已知被读过的格做阳性对照（rtrace 认不认得读）才算数 —— **尚未做，暂不作结论**。

**工具口径失误**：`--uart-log` 的端口参数要写 `uart1`，我写成 `1` 会掉进"其余分支"落到 uart2（0 字节），
**产出空文件且不报错**。已并入探针伪因清单。

### 8.49 第 23 轮定案：那次"停机"是固件主动走 `board_reset` 复位，不是崩溃

**先记一条自己的取证失误**：站点我挂成了 `--site 36EB30`，以为那是 `FUN_0000b030` 的入口。
按别名回算 `0x36EB30 − 0x363AE8 = 0xB048` —— 那是**循环回边**（`subs r4,#1 / bne`），
4 次预算全被回边吃掉，现场什么都没拿到。真入口 `file 0xB030` 的别名是 `0x36EB18`。
（同一地址口径的错第三次发生，进清单。）

用正确地址重跑，第 **5,474,668,120** 条抓到入口现场：

```
[site 0036EB18] sp=3C5BF2B0 lr=0036A867 r0-r12: 00000000 00000000 3C5BF284 00000004 00000001 ...
  mem@3C5BF2B0: 00000000 0C504A6B 00000036 00801F08 00001000 00000002 00001000 0036EB8B 02210000 0C50ECFB ...
```

**尾段的真实结构**（逐条核过；顺带纠正 §8.46 里"唯一入口是 0xAF20 的 beq"那句 —— 那是另一条尾段）：

```
0xB02A  bl FUN_0000aa28        0xB02E  b .          <- 第一条 halt
0xB030  bl 0x00006558          ; 刷串口输出
0xB034  movs r0,#0x50 / bl 0x0000c0a8                ; 忙等
0xB03A  movs r4,#0xa
0xB03C  bl 0x00006558 / 0xB040 movs r0,#0x10 / 0xB042 bl 0xc0a8 / 0xB046 subs r4,#1 / 0xB048 bne
0xB04A  bl FUN_0000aa28        0xB04E  b .          <- beat 看到的那条 halt
```

`lr=0x36A867` 与 beat 的 `LR=0x36E5A7` 都能逐位对回去：后者正是 `FUN_0000aa28` 内
`bl 0x0000B9EA @0xAABA` 的返回地址（`alias(0xAABE) = 0x36E5A6 | 1`）—— 链路自洽，不是猜的。

**`FUN_0000aa28` 就是 PMU 复位/断电请求**：里面 `movw r1,#0xCAFE` 与 `movw r1,#0x5AFE` 配
`mov.w r0,#0x3000`（BES 的 PMU 口令解锁/回锁），再写 `0x3048=0x2000`、`0x304D=2`、`0x3036=2`、
`0x3043 |= 0x6000`。同段 SRAM glue 的名字池（file 0xD900..0xDAE0）把语义直接写出来了：

```
'Start pmu %s'   +  'shutdown' / 'reboot'
'Error: pmu wdt reboot failed!'      'Error: pmu shutdown failed!'
'Error: SOFT_POWER_OFF=1!'           '%s boot_cause=0x%04X first_pwron:%d'
```

⇒ 真机上 PMU 会把芯片复位/断电，模拟里没有这一步，固件就只能死等。

**调用者是 NuttX 的 `board_reset()`**（本轮关键收获）：栈上 `0x0C50ECFB` → file 0x44ECFA 落在
`FUN_0044EC4C` 里，而它的字面量池就是 `board_reset` 的打印串：

```
"reset mode:%d"      "Dump board_reset backtrace:"
"Version: %s %s %s %s %s"   "Time: %s"      "/system"  "/data"  "xt_test_log"
```

`board_reset` 全镜像只有两个调用点：file **0x40355C** 与 file **0x4373FA**。后者在 `FUN_00437394`
—— 一张 `tbb [pc,r0]` 的 10 档分派表（`boardctl()` 那一类），两条相关分支是

```
movs r0,#3 ; bl 0x004037B0 ; bl 0x0044EC24        ; 另一条尾段（疑似关机）
movs r0,#1 ; bl 0x004037B0 ; bl 0x0044EC4C        ; board_reset(mode)
```

即"先做一次收尾/落盘，再复位"。

**判据因此再收一格**：要答的是"谁调了 `board_reset`、mode 传的是几"。站点已挂在
`board_reset` 入口 `0x0C44EC4C`（`--site-after 5G`），同一条跑里 `--ramdump 3C5BF000 500` 落栈窗口。

**顺序问题定了**：复位在 5.47G，帧请求在 257M 与 2.47G —— UI 还没轮到出帧系统就决定复位。
所以 §8.47 的 vsync/TE 那条先放着，**先解决复位**，否则两条原因分不开。

### 8.50 第 23 轮再进一层：那条"复位"是 NuttX panic 现场转储之后的重启

先把一次自己的取证失误钉住（第三次同一口径）：站点我写成 `--site 0C44EC4C`，
而 `board_reset` 在 file 0x44EC4C ⇒ 运行地址应当是 `0x44EC4C + 0x0C0C0000 = 0x0C50EC4C`。
`0x0C44EC4C − 0x0C0C0000 = 0x38EC4C`，roundtrip 一对就知道挂错了，**零命中是口径错不是没执行**。

**栈窗把链条钉死了**（`--ramdump 3C5BF000 500` = `build/p65_scan/halt_stack.bin`，逐字扫代码域）：

| 栈上字 | 运行地址 | file | 落在谁体内 |
|---|---|---|---|
| `0C4C3561` | 返回地址 | 0x403560 | **正是 `bl board_reset @file 0x40355C` 的下一条** |
| `0C4F74F3` | 返回地址 | 0x4374F2 | 正是 `bl 0x403330 @file 0x4374EE` 的下一条 |
| `0C50ECFB` | 返回地址 | 0x44ECFA | `board_reset` 体内（`bl 0x444A54 @0x44ECF6` 之后） |
| `0036EB37` | 别名 | 0xB04E | PMU 尾段 `bl FUN_0000aa28` 的返回地址 |
| `0C4CA4BD` | 返回地址 | 0x40A4BC | 又一个 `bl 0x4374E8` 档位的调用者区 |

于是链条是：**某处 → `FUN_004374E8`（noreturn 尾段）→ `FUN_00403330`（打印现场）→
`board_reset(mode=2)` → `pmu_reboot` 尾段 `b .`**。
`up_reboot` 体内末尾就是 `FUN_0044EC4C(2)`，而 `board_reset` 里 `cmp r4,#0x2` 有专门分支 —— mode=2 对上。

**`FUN_00403330` 的真实身份 = NuttX 的 panic 现场转储**，它的字面量池说明一切：

```
'dump_tasks'      'dump_stacks'      'User'   'Kernel'
"%s:    PID GROUP PRI POLICY   TYPE    NPX STATE   EVENT      SIGMASK  STACKBASE  STACKSIZE  USED ..."
"%s:   ----   --- --- -------- ------- --- ------- ---------- ---------------- %p   %7u   %7zu   %3zu.%1zu%%%c"
'%s: ERROR: Stack pointer is not within the stack'
```

**为什么串口上什么都没有**：这些打印走 `FUN_00444A54(0, fmt, ...)`（file 0x444A54 = 运行 0x0C504A54，
r0=level、r1=fmt）——**延迟日志**，要 logd 那条任务把缓冲搬出去才见得到。本轮 UART1 从 2.4G 起就停在
3216 字节不再增长，说明那几行现场根本没被搬出来。所以"日志没报错"不等于"没报错"，
判据要换成**直接挂日志包装函数抓 fmt 指针**。已按这个口径重跑（站点 `0x0C504A54` + `0x0C4374E8`，
`--site-after 5.4G`、预算 16、`--site-mem sp` 取 varargs），断言那条格式串是
`'Assertion failed %s: at file: %s:%d task: %s process: %s %p\n'`（池 file 0x198EB0、
PSRAM VA 0x3C1690EC）—— 抓到 r1 == 这个值就能把 file/line/task 一并读出来。

**镜像里另有 PANIC 一族串**（都在池、无静态引用，符合 p65 名字池零引用的老规矩）：
`'PANIC!!! Hard Fault!:'`、`'PANIC!!! Bus Fault:'`、`'PANIC!!! Memory Management Fault:'`（file 0x1B25CC 起）。
到底是软件断言还是硬件 Fault 之后主动重启，**还不能只凭"异常进入/返回配平"下结论**
（handler 走完再重启也会配平）—— 由上面那次抓 fmt 指针的跑来决定：r1 落在
`Assertion failed ...` 就是断言，落在 `PANIC!!! ...` 就是硬件异常。

### 8.52 第 23 轮：panic 点定到行号——`miwear_init.c:187` 的启动看门狗（阈值 30001 ms）

**抓法**（可复用，比猜调用者便宜）：站点挂在延迟日志包装 `0x0C504A54`（file 0x444A54，
r0=level、r1=fmt、后面的参数在 r2-r4 与栈上），`--site-after 5.4G`。16 次命中把 NuttX 的
转储台词全打出来了：

```
%s: Current Version: %s %s %s %s %s
%s: Assertion failed %s: at file: %s:%d task: %s process: %s %p     ← r2=_assert  r3=panic
%s: R0: %08lx R1: %08lx R2: %08lx R3: %08lx      （up_dump_register 一族：R4-FP / R8-R11 /
 xPSR-BASEPRI-CONTROL / EXC_RETURN）
%s %s Stack:  /  base: %p  /  size: %08zu  /  sp: %p                 （dump_stackinfo）
%s: %p: %08lx ×8                                                     （stack_dump）
```

⇒ **panic 现场其实被完整生成了，只是从没搬到串口**（logd 那条任务没搬缓冲，UART1 停在 3216 字节）。
"串口没报错"这条判据在 p65 上不可用，必须直接挂日志包装。

**断言参数**（站点寄存器现读）：`r2 = "_assert"`、`r3/r6 = "panic"`（NuttX 用字面表达式 `"panic"`
表示主动 `PANIC()`）、`r5 = 0x3C2AC2A8 = /home/work/data/miui_codes/build_home_rom/apps/vendor/xiaomi/miwear/apps/system/miwear_init.c`。

**定位手法**：那两个参数字符串的 VA 在镜像里**各只有一处字面量**（`re` 扫 4 字节小端值）——
`file 0x86C2E8 = "panic"`、`file 0x86C2EC = miwear_init.c 路径`。于是 panic 点直接落到它们中间的代码：

```
0086C2C8  movs r1,#0xbb            ; 行号 = 187
0086C2CA  ldr  r2,[0x0086c2e8]     ; "panic"
0086C2CC  ldr  r0,[0x0086c2ec]     ; miwear_init.c
0086C2CE  bl   0x004374e8          ; 断言尾段（dump → board_reset(2) → pmu 复位）
```

**为什么走到这儿**：0x86C224..0x86C296 是一串 64 位乘比较（`umull/adc/sbcs`，乘数
`0x83126E96`/`0x83126E97` 是除以 1e6 的魔数倒数），拿**当前时间**与 PSRAM
`0x3C479B08` 里存的 64 位时间戳（`ldrd r6,r5,[r4]`）比：

```
0086C296  bcs 0x0086c2b6           ; 当前 >= 上次记录 → 进差值判定
0086C2B6  movw r1,#0x7531          ; 30001  ← 阈值
0086C2C6  bcc 0x0086c29c           ; 差 < 30001 → 更新时间戳、继续等
0086C2C8  ...PANIC()               ; 差 >= 30001 → 主动 panic
```

同一个块里还把 `0x493E0 = 300000` 当参数写进栈（`strd r4,r5,[sp]`）。
⇒ **这是 miwear_init 的启动看门狗：30001 个毫秒单位内没有"进展"就自己 panic 重启**，
而模拟里它到期了 —— 也就是说 `board_reset` 不是崩溃后果，而是固件按设计超时自杀。

**顺带一条给字体线的收获**：同一函数族的池里（file 0x86C1D0..0x86C1E8）有
`MiSans-Regular` / `MiSans-Medium` / `MiSans-Demibold` / `MiSansF-Regular` / `MiSansF-Medium` /
`MiSansF-Demibold` / `N62SFontVF` —— p65 的字体登记就在这一段（`bl 0x7FD1D8` 按名字找对象），
Chaos 的字体投递在 watch6 上要对的就是这批名字。

**判据顺序定了**：要出第一帧，先让这条 30 秒看门狗不塌 —— 即把 miwear_init 在等的"进展"补上。
候选：sensor 核/rpmsg 服务、蓝牙栈、watchface/UI 服务的 ready。正在用同一挂法抓 panic **之前**
（4.6G 起）的延迟日志，把它到底在等什么点名出来。

### 8.53 第 23 轮：这条看门狗不是模型的伪命题（时间基算过了）

先算账，再决定要不要去补什么对端应答器 —— 因为"30 秒到没到"在模拟里必须能核：

- 站点口径：日志包装 `0x0C504A54` 从 **4.6G 起挂、到 panic（5,471,696,500）零命中**，
  83 次命中全部是 panic 自己的转储。⇒ 系统是在**沉默**里等满看门狗的，不是先报错了再重启。
- miwear 任务在 `rcS` 里最后创建（日志 `miwear [52:104]`，对应约 2.4G 前后），
  到 5.4717G 过期 ⇒ 等待时长约 `3.07G 步 / 30001 ms` ≈ **10.2 万步每毫秒**。
- 对照硬件口径：best1502p 主频量级 200 MHz 上下，解释器 CPI≈2 ⇒ 每毫秒约 10 万条指令。
  两个数对得上 ⇒ **模拟里的"30 秒"就是真机意义上的 30 秒**，看门狗到期是真的等不到东西，
  不是我们把时间走得过快。

推论（下一轮据此排序，别重复算）：
1. 要挡住的不是时间基，而是"miwear_init 在等的那个进展"。它等的是**别的模块的状态**，
   而 panic 前一条日志都没有，说明它只是安静地轮询/睡眠。
2. 显示链那条仍然可疑且**与它同源**：帧请求置了标志（257M、2.47G）但没人搬帧，
   `lcdc 中断 pend=0`、`frame_start(+0x224)` 与 `scanout_en(+0x280)` 都没被写、
   `screen_turn_on(0x0CA69200)` 在 8G 内从未命中。若 UI/显示服务卡在"等第一帧的 vsync"或
   "等 `screen_turn_on` 那三道前置门"，miwear 的启动进展就永远不完整 ⇒ 两条是同一条链。
3. 时间基这条**结掉**（不要再拿 `--reboot` 去绕：绕过一次也只是把同一次 30 秒等待再走一遍）。

### 8.54 第 23 轮手法升级：把固件日志从"看不见"变成"全量收割"

p65 的打印走**延迟日志**：包装函数 `0x0C504A54`（file 0x444A54，`r0=level`、`r1=fmt`、其余参数在
r2-r5 与栈上）。它只把格式串和参数登记下来，真搬到串口要 logd 那条任务跑 —— 本轮 logd 没跑，
所以 UART1 停在 3216 字节、看起来"什么都没发生"。做法：**直接把站点挂在包装函数上**，
`--site 0C504A54 300 --site-after <N>`，再用池口径把 `r1` 解回字符串
（`file = VA − 0x3C000000 + 0x2FDC4`）。一次 6G 跑收满 125 条。

**2.47G → 2.495G 的开机尾段**（调用者 file 全部现读，格式串原样抄）：

```
0x86CE56  [%s] %s: init state -> %d                         miwear_init 的状态机（与看门狗同模块）
0x89084C  show hyperOS                                      show logo 那一族（0x890818 附近）
0x8723F6  [%s] %s: set system TZ [%s].
0x8724CE  [%s] %s: Softweare Version: %s                   （固件里就是这个拼写）
0x97E164  [%s] %s: V_11-08_12-00                            闹钟时段
0x98212C  [%s] %s: [%s] get file dir state failed, errno = %d        ×2
0x97D5EA / 0x97E2D2 / 0x97E472  alarm_id=[%llu] type=[%d] timeout[_ms]=[..]   闹钟登记
0x981DD4  [%s] %s: failed to get initial off body status     健康算法初值拿不到
0x981DF0  [%s] %s: initial wrist wear status:%s
0x981E2E  [%s] %s: initial algo sleep status:%s
0x9A9CA0  aod_mode=%s aod_begin_time=%02d:%02d aod_end_time=.. aod_attr=%x
0x9A9CD2  wrist_tilt_mode=%s .. wrist_tilt_attr=%x
0x9A9D16  touch_double_click_enabled=%s slide_up_enable=%s touch_palm_enabled=%s touch_attr=%x
0x9A9D32  auto_brightness_enabled=%s     0x9A9D44  brightness_value=%d
0x9A9D58  dimmer_timeout_milliseconds=%d
0x9AA194  [%s] %s: failed to open %s, errno %d               有一个文件没开成（参数在栈上，待取）
0x9A9F34 / F56 / F8C / FC6 / FFC / 030  tag=%s handle=%p status=%s  ×6   订阅登记
0x9A706E  tag= %s        0x9A70AA  succeeded to subscribe wrist_tilt
0x9A7368  ## wrist_tilt_alarm_time %04d%02d%02d%02d%02d%02d   ×2
0x9A7D0C  [%s] %s: turn on screen, tag=%s                    ★点屏请求确实发出去了（2.4923G）
0x9A76A0 / 0x9A76C4  tag:%s, normal_stay_count:%d            会话计数那道的计数在动
0x9A79AC  keepon_enabled:%s, keepon_duration_enabled:%s, duration: %lu
0x9A68A2  full_power_enabled:%s, auto_brightness_enabled:%s, brightness_value:%d
0x9A6962  touch_double_click_enabled:%s, touch_slide_up: %s touch_palm_enabled:%s
0x9AA0EA  g_screen.screen_status_topic_fd=%d
0x9768F8  [miconnect] [%s:%d]start xpc client success         ← 最后一条（2.4951G）
```

之后到 5,471,696,500（panic 转储）**一条都没有**。

**panic 现场额外收获**：`backtrace|%2d:%s`（file 0x43D62E）24+ 行、任务表
`%s:    %4d %5d %3d %-8s %-7s %c%c%c %-18s %08lx%08lx %p ...`（file 0x403302）24 行、
`Remote: %s headrx %u`（file 0x412D08，rpmsg 侧在复位前被点名两次）、
`reset mode:%d` / `Dump board_reset backtrace:` / `Version: %s %s %s %s %s` / `Time: %s`。
⇒ 任务表里 `STATE/EVENT/COMMAND` 三列就是"谁卡在等什么"的答案，参数在栈上，
下一轮用 `--site-mem sp` 定点取那 24 次命中的 varargs 即可（比建对端应答器便宜）。

**要显式纠正的一条旧记录**：§8.42/地址表里写着 `screen_turn_on(0x0CA69200)` 8G 内零命中，
我一度据此写"点屏没走"。本轮反证在案：**显示服务确实打了「turn on screen, tag=%s」**（0x9A7D0C，
2.4923G），而且第二次帧请求命中就在 2.4767G —— 同一个时间窗。
⇒ 正确说法是"点屏请求进入了显示服务、并且登记了出帧请求，但没走 `0x0CA69200` 那个入口函数"
（它 15 个 bl 调用点是另一条唤醒路径）。之前那句"点屏没走"收回。

### 8.55 第 23 轮：断言入口现场（与静态逐位对上），并记一条"栈扫描只给候选"的规矩

站点这次按换算器给的运行地址挂（`file 0x4374E8 → 0x0C4F74E8`，`to` 模式回代自证过），
第 **5,471,694,609** 条抓到断言尾段入口：

```
lr=0C92C2D3  sp=3C5BF4C0
r0=3C2AC2A8  miwear_init.c 全路径
r1=000000BB  187            ← PANIC() 的行号
r2=3C1693BC  "panic"        ← NuttX 用这个字面表达式表示主动 panic
r3=00007531  30001          ← 看门狗阈值
r4=3C479B08  时间戳格
r6=0000608B  24715          （原始读数，含义未定，别拿来下结论）
mem@3C5BF4C0: 00000000 00000000 3C3D467C 00000000 3C3D4660 3C5BF4E8 00000000 3C3D4674
              00000080 0C4CA4BD 00000000 00000001
```

`lr=0x0C92C2D3` 正是静态找到的那条 `bl 0x4374E8 @file 0x86C2CE` 的返回地址 —— 现场与 §8.52 的
静态定位**互相对上**，这条链不再是推的。

**记一条规矩**：栈里那个 `0C4CA4BD` 解回 file 0x40A4BC，落在 NuttX 的信号量 post 实现
（`FUN_0040A45C`：走等待链表、`blx r2` 唤醒，返回地址就是 0x40A4BC）里。看着像"看门狗是被
一次 sem post 唤醒后超时" —— **但栈上的代码域数值只是候选，不是证据**：`blx` 返回之后那一格
就变成过期数据，后面谁复用同一段栈都会留下别人的返回地址。
所以这一层不下结论，改判据：**去读固件自己算出来的任务表**（panic 转储里
`%s: %4d %5d %3d %-8s %-7s %c%c%c %-18s ...` 那 24 行，STATE/EVENT/COMMAND 三列会点名
"谁卡在等什么"），参数就在日志包装的栈 varargs 里，`--site-mem sp` 定点取。

另外把 §8.54 里那条状态机读数补齐（同一次抓法的寄存器现读）：
`init state -> %d` 那一条的 `r2="init"`、`r3="init_state_transist"`、`r4=0x3C479C6C`（状态字节格），
`%d` 的实参走栈（`str r1,[sp]`，r1 = `ldrb [r4]`）。`FUN_0086CE28` 打完就把状态 **+1** 写回
（`adds r3,#1; strb r3,[r4]`），随后会打 `[%s] %s: miwear exit`（tag=`miwear_main`）。
本轮实测：**状态迁移只发生一次**（2,470,811,051 条，state 0 → 1），`miwear exit` 从未出现
⇒ miwear_main 进了主循环就再没往前走过一步。

### 8.56 第 23 轮落地：DSI 面板命令接上状态机 + TE 脚改成有证据的 59 —— TE 沿第一次真正进入固件

**改动一：面板命令走 DSI，也得喂给同一个状态机。**
写引擎（file 0x8ADA0C）的包格式现读出来了：

```
短包（1 参数）：+0x18 = (命令<<8) | (参数<<16) | 0x15     ← len==1 支路
长包：          +0x18 = (长度+1)<<8 | 0x39，命令是随后送进 +0x14 的第一个字节
读侧仍是 (大小<<8)|0x37 与 (命令<<8)|0x06 —— 与写侧不撞。
```

p65 实测下发的 12 条写命令里**第 4 条就是 `0x35`（tear-on）**（第 214,515,382 条），
还有 `0x51` 亮度（0x1D→0xFF）、`0x53`、`0x63`、`0x67`、`0x68`、`0xFE`、`0x04/0x0A/0x0E`（DDB 读）。
我的模型原来只在 **LCDC 自己的命令引擎**里认 0x35/0x34，而 p65 的 `lcdc 命令=0`、
命令全从 DSI 走 ⇒ `te_enabled` 永远是假、TE 沿永远不发。现在：

- `fl_dsi` 解出写短包/长包的命令，经 `fl_dsi_set_panel()` 交给装配方接的口；
- 装配方（`test_boot.c`）把口接到 `fl_lcdc_panel_command()`；
- `lcdc.c` 把原来写在 `do_command` 里的面板状态机抽成公开函数，两条来源喂同一份状态。
- 单测补 `[D6]/[D7]`（短包 0x35/0x51 解码、长包 0x11 解码、计数），sabotage 对照组同步加；
  全量 `bash flint-vm/build.sh` 通过（DSI 71 项、全部对照组成立）。

**改动二：TE 脚从 47 改成 59（有证据，不是猜）。**
400M 条跑到面板初始化之后，固件自己开的**唯一一条 GPIO 中断**是 bank1(40089000) bit27
⇒ 脚号 = 32 + 27 = **59**；bank0 全 0。`watch6-p65.board.json` 里原来的 47 是从 p67 板照抄、
自己就标着"待核"。已改 59 并写 `_te_gpio_note`（等级：推断待核，判据=TE 沿被接走且随后写 +0x224）。

**改后实测（1.5G 跑）**：

```
gpio 沿31 次（门没开挡下0）ack122 次｜中断号=44 pend31 次
C 段：GPIO 现场（垂直同步给了 31 次沿、屏没要跳过 5 次；脚 59 的沿 31 次 / 应答 30 次）
```

—— **TE 沿第一次真正进到固件**（原来 `te 给=0 跳=192`）。但 `lcdc 帧=0` 仍然：

**新的定性（这是下一步的抓手）**：帧请求只把 `obj+0x148=1`、`obj+0x154++` 记上，
真正的搬帧消费者在同模块 file **0x8A8A64**：

```
if (!g_enable) return;
FUN_0040725C(obj+0x160, 0);
obj+0x15d = 1; obj+0x179 = 1;
loop:  r3 = obj+0x148;
       if (r3 > 0 && FUN_00406DE0(obj+0x160, 0xAA) == 0) goto loop;   // 等信号量带超时
       r1 = obj+0x140; r1 = (r1*17000 + 99)/100; ...
```

它唯一的调用者是 **file 0x44F63C（fb 的 ioctl 0xFB 处理）**，而 ioctl 0xFB 在 8G 内一次都没被调用
⇒ **渲染器把帧标好了，但"谁来 flush"那一步（用户态显示服务发 ioctl 0xFB）从未发生**。
rtrace 取证（阳性对照已做：同一开关能报出 `40230010` 的 6 次读与 PC）：
`obj+0x148..+0x157` 在 250M→800M 之间**只有生产者自己那两次读写**
（`3C3CA480` 宽1 @PC=0C9689DA、`3C3CA484` 宽4 @PC=0C9689F2），没有第三方消费者。
⇒ 下一步要么找出"该发 ioctl 0xFB 的那个显示服务任务为什么不动"，要么证明它等的信号量该由谁 post。

### 8.57 第 23 轮：开机 0→3 亿条的全量日志（挂日志包装整段收割）

同一挂法（站点 `0C504A54`、`--site-after 0`、预算 600，一次短跑），106 条按顺序落下来。
下面只抄**有判读价值**的（格式串原样、调用者 file 现读）：

```
0x4E8353  [FS1818]I2C read err %d                                   ×2（音频 PA 探测失败）
0x4E99E5  [FS1818]Audio: id=0x[%08x] vs. 0x%08x[fs1818] vs. 0x%08x[fs1896]
0x4E99F9  [FS1818]Audio: fs1818_check_id Matched: failed
0x9509B5  xt_ota %s===%d   size = %ld                              （ptable_ftl_nand_flash_2K_init）
0x4DE5A3  read g_lcd_module_id is 0x%lx!          → 实参 0x200145（rm690c0-VSN 的 145 株）
0x4E330F  init lcd rm690c0!
0x4E33C1  ######vsn p01
0x4DE617  init rm690c0 lcd finish!
0x512D1B  up_fbinitialize=0x%x,0x%x                                 ×2
0x968321  Initialize LCDC driver fbmem:%p,fbsize:%d
0x968363  Configuring the LCD controller
0x96836B  Configure lcd periphery
0x96837F  Configure global register                                ×2
0x96838B  Initialize lcdc layer
0x968397  Enabling the display                    ← 215,012,151 条：固件认为"显示已使能"
0x9683E3  lcdc_initialize done
0x9500B9  SoftWare Version: %s
0x4DE173  [CHARGER][BESPOWER]cxc i2c%d initialize succeed!
0x4F4453  [CHARGER][SY6103X]sy6103x_initialize
0x4F4587  [CHARGER][SY6103X]THIS IS NO SY6103                    ×3
0x4DE1C5  [CHARGER][BESPOWER]sy6103x initialize failed
0x4F521B  [CHARGER][SC7061]sc7061_initialize
0x4F5325  [CHARGER][SC7061]THIS IS NO SC7061                     ×4
0x4DE20B  [CHARGER][BESPOWER]sc7061 initialize failed
0x4DE1F3  [CHARGER][BESPOWER]not find charger ic
0x950215  [CHARGER]find changer failed                            （固件原文就是这么拼）
0x4F6579  [BAT]cap delay us:%lld
0x4F6B59  [ADC VALUE] time:%d, value:%d                            ×9（电池 ADC 采样）
0x4F6293  [BAT]charg %d volt %d shutdown threshold %d
0x4F6C29  %s,board_id=0x%x                                          （read_board_id）
0x4F5875  [BAT][batterry_log]voltage1:%d, level=%d, state:%d, shell:%.2f, skin:%..
0x4F731F  [batterry_log]:vbus=%d
0x4F6017  [BAT]pmu_charger pin change = %d
0x4F6179  [BAT]high_temp_end_charger--display--
0x4F68F5  [BAT]HIHG_TEMP_STOP_CHARGER_STATUS
0x4F641D  [BAT]current_area_block=%d voltage=%d
0x9501F5  %s, data.header error                                     （load_nor_driver_data）
0x950165  %s,reverse=%d, level=%d
0x95082D  show logo                                                  ← 257,731,918
0x4F1B89  [AW86225] board_aw86225_initialize                        （线性马达）
0x4F1BFF  [AW86225] i=%d
```

（`show logo` 之后 0.4M 条就是帧请求命中点 257,732,346 —— 两件事对得上。）

**从这份表里读出三条新事实**：

1. **LCDC 的"Enabling the display"发生在 215,012,151 条**，而 `+0x280` 一次都没被写
   ⇒ 我按参考实现命名的 `scanout_en(+0x280)` **不是 p65 的使能格**。p65 实际写的是
   `+0x21C/+0x220/+0x228` 那几个我没命名的格（值 0x05020020 / 0x001F001E / 0x1A000000）。
   别再拿 +0x280 当"没使能"的判据；模型的寄存器命名要按 p65 的实际写序列重核。
2. **充电 IC 两个候选都没认出来**（SY6103X / SC7061 都判 "THIS IS NO …"），
   固件最后 `not find charger ic` + `find changer failed`，随后电池只能靠 ADC 采样
   （`[ADC VALUE]`、`[BAT]charg … volt … shutdown threshold`）。这是已知的 i2c0@7 寄存器值不匹配，
   现在有了完整台词与时刻（216.8M–218.7M）。
3. **触摸驱动（Hynitron/CST92xx）在 0–300M 一条日志都没有**：`[HYN][%s]%start driver init.`
   没出现。而 2.489G 时 `MiWearScreen` 的 `screen_init` 报
   **`failed to open /dev/input0, errno …`** ⇒ 触摸设备节点根本没建出来。
   这就是"没人发 fb ioctl 0xFB、没人搬帧"的上游。

**因此下一轮的两个端点任务（按对首帧的贡献排序）**：

- **触摸 IC（第一优先）**：i2c2(40060000) 上对从地址 **0x40** 有 78 条命令、**全部 NACK**
  （设备窗口 dump 里 i2c2 的"命令78 应答0 无应答78，最近从地址 40"）—— 形状就是探测重试。
  要按 HYN 驱动的读序列（`read_chip_id`、`read 0x0002`、`partno_chip_type` 0xD204/0xD208 等）
  把端点建起来，先让 `/dev/input0` 出现，再看 `MiWearScreen` 能不能 init 成功。
- **充电 IC（第二优先）**：i2c0@7 已应答但寄存器值不匹配；要么把 SY6103X/SC7061 的 ID 寄存器
  按固件的比较常量填对，要么接受"无充电 IC"（ADC 路径仍在跑，但要盯 `shutdown threshold` 那一行）。

### 8.58 第 23 轮：给 p65 建 CST92xx 触摸端点（有公开模型做依据）——0x28 开始应答

**依据链**（三条独立证据合上）：
1. 固件自己的触摸驱动是 **HYN/CST92xx**：池串 `hynitron_worker`、`[HYN][%s]TP irq1`、
   `cst9217_ts_getstate`（file 0x423xxx 模块，`FUN_004238B8` 入口里 GPIO 0x1D 上电、PM 资源 0x24）。
2. 公开引擎仓 **`0x504B0304/bes-qemu`**（MiBandStudio 的 `release/build-info.json` 写明引擎就是它，
   设备目录里带 `redmi-watch6`）有完整的 `board-devices/touch/cst9217/` 模型与 README：
   selector 两字节、`0x0002` 回 mode<<8、`0xA002` 回探针字、`0xD204/0xD21C` 元数据、
   `0xD220` 模块号、`0xD224` 身份字 `0xCACA9217`、`0xD000` 十字节触点报告（无触点时第 6 字节 0xAB）。
3. 固件在 **i2c2** 上对从地址 **40（十进制 = 0x28）** 发了 78 条命令、全 NACK。

**一个自己犯的读数错**：i2c dump 的"最近从地址"用 `%u` 打印，是**十进制**。
我把 40 当成 0x40 去配端点，白跑一轮（`--i2c-trace 40` 也按十六进制解析，两个口径混在一起）。
反证：o66 那份的"地址 90"只有按十进制读才是 0x5A，正好对上公开模型 README 的
"the qualified S5 image probes I2C1/0x5a"。**现在 dump 改成 `%u=0x%X` 两个都打。**

**实现**（按本仓的四步规矩）：
- `i2c.c` 新增 CST92xx 端点：写相位攒 selector（RESTART 位与读命令都会重开地址相位），
  读相位按 selector 回 4 字节；`0xD000` 回 10 字节报告。身份/探针/元数据三个值**放进 profile**
  （`touch_addr`/`touch_reg_probe`/`touch_reg_meta`/`touch_reg_id`），src 里只留协议 —— 硬编码门拦过一次，
  这也是它该拦的。
- 新单测 `test_i2c_touch.c`（selector、身份、元数据、报告、工作模式），build.sh 里按 DSI 同款
  加 p65 的 sabotage 对照组；全量 `build.sh` 通过。
- 自测当场抓到真 bug：selector 不复位时，第二组寄存器读到的还是上一组的回值（9 条断言一起红）。

**实测（2.6G 跑）**：i2c2 从 `命令78 应答0 无应答78` 变成 **`命令5 应答5 无应答0`**，
事务记录：`地址28 发出 00 20 00 01 00`（PC=0C970844，通用 I2C 发送函数 `FUN_008B0738`）。
但 **HYN 的 worker 仍然一次没进**（站点 `0C4E38B8` 零命中），说明这 5 笔不是触摸驱动发的 ——
0x28 上可能还挂着别的东西（面板/背光那一族），调用者正在用站点取。
触摸驱动为何不 init，下一步从"谁注册 HYN 设备、`/dev/input0` 由谁 register_driver"去查。

### 8.59 第 23 轮：触摸驱动真的跑起来了——驱动协议逐笔对上，卡在 A018 身份字

把日志站点挂到 2.0G 之后（`--site-after 2000000000`），2.15G 前后拿到了触摸初始化的全过程：

```
2077104969  [touch_lowhalf]TP ZINITIX POWEER ON ...        → ts write reg failed
2078285900  [touch_lowhalf]ts write cmd failed!             → write 0x0000 fail → 重试两次
2129935020  bes_ts_zinitix_setup() failed: %d               （zinitix 先试、失败）
2140125533  TP HYNITRON POWEER ON ...%d
2140174069  [HYN][%s]dev: %p devname: %s     → devname = /dev/input0   ★
2152336556  [HYN][%s]%s:start driver init.  → 驱动版本串 CST92xx_MCU_Driver_V3.8_20240318
2167820806  [HYN][%s]partno_chip_type: 0x%04x   → 实参 0x0000
2167872490  [HYN][%s]module_id: 0x%04x          → 实参 0x0000
2167920806  [HYN][%s]partno_chip_type error 0x%04x
2168974223  [HYN][%s]read_chip_id fail,error return   ×2
2169024472  [HYN][%s]ERROR: Failed to attach interrupt
2169067301  bes_ts_hynitron_setup() failed: %d
```

**驱动与端点之间的逐笔事务**（`--i2c-trace 5a 80 --i2c-trace-after 2000000000`，address 0x5A）：

```
A0 01 AA            A001 带参数（写模式）
A0 02  → 读 55 B0      0xA002 探针字 = 0xB055  ★ 读对了
A0 01 00            A001 带参数
A0 10 01            ← boot 页 = 1
A0 0C 7C 07         ← boot 地址 = 0x077C
A0 04 E4 / A0 04    A004 状态（未建模，回 0，与公开模型一致）
A0 18  → 读 00 00 00 00   ★ 旧模型在 A018 回 0 ⇒ 身份字读成 0 ⇒ partno/module 都是 0
A0 10 00 / A0 0C C0 7F    ← 再试 boot 0x7FC0
```

⇒ 驱动的读法与公开模型 **cst9217.c 一字不差**：`0xA002` 探针、`0xA010/0xA00C` 设 boot 页与地址、
`0xA018` 读 boot 身份（页 1 / 0x077C 回身份字、0x7FC0 回 1）。旧模型没实现 A018，所以卡在
`partno_chip_type error 0x0000`。

**已补**：`i2c.c` 增加 `ts_boot_bank/ts_boot_addr` 的登记（A010 三字节、A00C 四字节）与
`0xA018` 的判据；单测补 `[T5]`（全量 `build.sh` 通过，触摸单测 83 项 + 对照组成立）。
**还没解决**：`Failed to attach interrupt` —— 驱动要挂触摸中断脚，模型里还没有这根 GPIO。
等 A018 过关后看驱动是先注册 `/dev/input0` 再挂中断，还是反过来；若是后者，就要把
触摸 IRQ 脚也建出来（公开模型用的是 active-low GPIO，p65 的脚号待定）。

### 8.60 第 23 轮里程碑：触摸驱动初始化**成功**，`/dev/input0` 这次开得开

补上 `0xA018` boot 身份后同一条 2.6G 跑，2.183G 处：

```
2165144591  [HYN][..]partno_chip_type: 0x9217      ← 身份字这次读到了
2165198207  [HYN][..]module_id: 0x0001
2170295941  [HYN][..]set_work_mode: ENUM_MODE_DEBUG_INFO
2173562612  [HYN][..]set_work_mode: ENUM_MODE_NORMAL
2176621397  [HYN][..]chip firmware_info_ok: 0x0001 ... version/project/checksum 全 0
2177762185  [HYN][..]upgrade_firmware_judge return,no need update fw.
2182870275  [HYN][..]*************DONE*************
2182915310  [HYN][..]hyn config pm=0
2182973386  hynitron_interrupt=0x1
2183045390  bes_ts_hynitron_setup() success        ★★★
2184717139  [HYN][..]TP irq1                       ← worker 起来并处理了一次中断
```

对比 8.59 那份失败日志：`partno=0x9217`、`module=1`、`chip id` 那几条错误全部消失；
`ERROR: Failed to attach interrupt` 也没了（`hynitron_interrupt=0x1` 表明中断接上了）。

**连带结果**：2.489G 前后原本那句 `[MiWearScreen] screen_init: failed to open /dev/input0`
**这次没有再出现** —— 说明 `/dev/input0` 已经注册、屏幕服务把设备开开了。
（本轮日志窗口从 2.0G 起挂，2.489G 在窗口内；同窗口里只剩 `chargerd` 打不开充电器设备
`open dev ... failed (-2)`，与"充电 IC 未被识别"一致。）

**新的小噪声**：2.484G 起 `[HYN][..]ESD data NA,need retry` ×4 → `esd_check power reset ic`
—— ESD 检查读的寄存器我们回 0，驱动会周期性复位触摸 IC。先记着，等首帧出来再决定要不要补。

**下一步判据不变**：`lcdc 帧>0`。已开 6G 跑，站点挂在
`0x0C50F63C`（fb ioctl 0xFB，真正的搬帧入口）与 `0x0CA689CC`（帧请求），
`--panel` 只有真出帧才写 PNG，观感判据从那张图来。

### 8.61 第 23 轮：触摸通了之后——看门狗不再触发，但帧仍卡在"请求挂着没人搬"

**6G 跑（触摸端点修好之后）**：运行到 6.0G 时 PC 仍在变（`0C4C66DC`），
**没有出现 5.47G 那次 `0x0036EB36` 冻死的停机** ⇒ miwear 的 30 秒看门狗这次没有到期，
触摸把"启动进展"补上了。这是本轮最实的进步。

**屏幕服务日志（2.4G 之后窗口）**：

```
2453807855  [%s] %s: g_screen.touchpad_dev_fd=%d      ← 触摸设备 fd 拿到了
2456569547  [%s] %s: turn on screen, tag=%s
2458484472  [%s] %s: g_screen.screen_status_topic_fd=%d
2459475571  [miconnect] [%s:%d]start xpc client success
```

**同一窗口里帧请求命中 1 次**（`0x0C9689CC`），渲染确实发生。但 3.2G 停机时把显示对象落盘
（`--ramdump 3C3CA330 1C0`）看到的是：

```
+140 = 00000000   +148 = 00000001   ← 帧请求挂着的标志
+14C = 3C3D54F8   ← 面板结构指针
+150 = 00000000   ← 那道 <2 的门（放行）
+154 = 00000002   ← 请求计数 2
+15D = 00000000   +179 = 00000000   ← 消费者的"在处理"标志从未置起
+1BC = 00000000   ← screen_turn_on 的前置会话计数（仍是 0）
```

即：**帧请求已经挂在对象上，搬帧消费者（`FUN_008A8A64`，唯一入口 `FUN_0044F63C`）
一次都没跑**。`FUN_0044F63C` 没有静态调用者，也没有函数指针字面量 —— 它是经表/句柄间接调的
消息处理函数（按 `r1[1]` 的 0xF0..0xFF 档分派），不是 POSIX ioctl。下一步要定的是
**谁该给它发 0xFB 那条消息**，以及显示服务任务此刻在等什么（帧请求现场的栈正在抓）。

`--panel` 因此没有 PNG（0 帧）。判据仍是 **`lcdc 帧>0`**。

### 8.62 第 23 轮：把"谁来搬帧"这一跳彻底定死——是 **DSI 驱动的 ioctl 0xFB**，从没被调用

把 `FUN_0044F63C` 反编译出来看清楚了（它不是 I2C、也不是普通 fb ioctl，是 **DSI 设备**的命令处理）：

```
cmd 6            → FUN_008AD7E0(...)     DSI 读引擎（我们早已建模的那条）
cmd 5/0x15/0x39  → FUN_008ADA0C(...)     DSI 写引擎（短包/带参/长包）
cmd 0xFB         → bl 帧请求(0x8A89CC) + bl 搬帧 worker(0x8A8A64)   ← 就是这一档
```

`r1` 是消息头：`[1]`=命令字节（0xF0..0xFF 走 tbb）、`[2]` 是标志、`[8]`=数据指针、`[0xC]`=长度。

**动态证据（3.2G 跑）**：
- 站点 `0x0C50F63C`（这个处理函数的入口）**零命中** ⇒ 从来没有人给它发过 0xFB；
- `--wtrace 3C3CA490 4 40 0`（搬帧 worker 等的信号量 `obj+0x160`）**只在第 10,732,536 条被 crt0 清 .bss
  写过一次**（PC=0C0C034E），之后再无写入 ⇒ 唤醒源也不存在；
- 显示对象现场：`+0x148=1`（请求挂着）、`+0x154=2`（两次请求）、`+0x150=0`（门放行）、
  `+0x15D/+0x179=0`（worker 从没进过）。

**帧请求现场**（第 2,441,173,959 条，lr=0x0C95055B，即渲染器 `FUN_00890470`）：
栈里 `0C4F8BB3` 解回 file 0x438BB2，落点函数是 `FUN_00438B84`（一个带重试计数的分派），
但那是 `bl 0x407178` 的返回地址、属于**同栈残留**，只能当候选（见 8.55 的规矩）。

**结论**：渲染只把"脏"标记写上（`+0x148`），真正的提交要靠**显示服务对 DSI 设备发 ioctl 0xFB**；
这一步在当前模拟里从未发生。下一步两条：
1. 找 DSI 设备节点与它的 `open`/`ioctl` 使用者（是哪个用户态任务在 flush 前打开它）；
2. 若显示服务确实在等外部事件（比如 sensor 核的 rpmsg 数据），就把那条对端补上。

### 8.63 第 23 轮：屏幕"请求点屏"之后——背光门与会话计数都还是 0

3.2G 时的落盘：
- 显示对象（0x3C3CA330）：`+0x148=1`（帧挂起）、`+0x154=2`、`+0x15D/0x179=0`、`+0x1BC=0`；
- 面板结构（0x3C3D54F8，`--ramdump` 0x430 字节）：**`+0x410/+0x411/+0x412` 全是 0** ⇒
  背光门的 `(v-2)<2` 不成立，真背光寄存器从未被写。

而屏幕服务在 2.4566G 确实打了 `turn on screen, tag=%s`，它所在函数（file 0x9A7CF0 起）后面做的是：
把 tag 存 `[服务对象+0x5D8]`、取时间存 `+0x5E0`、调 `0x9B3638`/`0x9A7674`/`0x9B35D0`、
把计数写 `[服务对象+0x630]`、把时间基写 `[服务对象+0x458]` —— 全是**服务自己的对象**，
不是显示对象 0x3C3CA330。

注意别把两个 `+0x1BC` 搞混：`FUN_009A7674` 里写的是**服务对象**的 `+0x1BC`（会话计数），
而 `screen_turn_on` 的前置门读的是**显示对象** 0x3C3CA330 的 `+0x1BC`。后者在 3.2G 仍是 0
⇒ `screen_turn_on` 那次调用确实没发生（与该入口 8G 内零命中一致）。

**下一步（按证据强度排）**：
1. `FUN_009A7674` 写服务对象 `+0x1BC` 之后，服务有没有走到"调 screen_turn_on / 提交帧"那一步 ——
   从该函数继续往下读，或直接在 `0x0CA69200`（screen_turn_on 入口）挂点等它命中；
2. 背光门 `+0x411` 的写者（谁把它置 2/3）—— 这决定屏是不是被判成"亮着"；
3. 帧提交仍是 DSI 的 FBIO_UPDATE(0xFB) 那条（见 8.62），未被调用。

### 8.64 第 24 轮：screen_turn_on 定名与两处撤销；搬帧唯一入口是消息 0xFD 而不是 0xFB

先记录两条**我自己之前的错**（纪律 18）。

**撤销一：8.63 里"`+0x1BC` 是 screen_turn_on 的前置门、为 0 所以点屏没发生"是错的。**
把 `0x009A9200` 整段读出来（Ghidra，`fn 0x9A9200`），它的字面量池直接给了自证：

```
0x009A9258  0x3C496800   ← 它操作的对象基址
0x009A925C  0x3C353D64   "screen_turn_on"
0x009A9260  0x3C352F50   "MiWearScreen"
0x009A9264  0x3C3538F0   "[%s] %s: system is shutting, refuse to turn on screen, tag=%s"
```

函数体：

```
009A9204  ldr r5,[0x009a9258]      ; r5 = 0x3C496800（屏幕服务全局块 g_screen）
009A9208  ldr.w r3,[r5,#0x1bc]
009A920E  cmp r3,#0x0
009A9210  bgt 0x009a923c           ; 已有会话 → 走 0x009A7878 那条
009A9214  bl  0x009a2304           ; system_is_shutting()
009A9218  cbnz r0,0x009a9228       ; 在关机 → 打上面那条错误日志并返回
009A921E  bl  0x009a7cf0           ; 真正点屏
```

所以 **`g_screen+0x1BC == 0` 不是拦路的门，而是"当前屏灭 ⇒ 正是走真点屏支路"的判据**。
而且它读的是 **0x3C496800**，跟我 8.61/8.63 dump 的显示对象 **0x3C3CA330** 是两个不同的块
（后者是 `FUN_008a8a0c` 那份显示设备对象）。8.63 把两个 `+0x1BC` 的"谁是谁"写对了，
但把 0x3C3CA330 那份 `+0x1BC` 当成了 screen_turn_on 的门，这一句作废。

`0x009A9200` 的静态调用者有 **19 处**（`xref 0x9A9200`，全是 bl），说明它是通用点屏入口，
不是某条冷支路。

**撤销二：8.62 说 `FUN_0044F63C` "没有函数指针字面量"，错了；它是一张 ops 表的第三项。**
全镜像按 4 字节搜 `0x0C50F63D` 命中 1 处，落在 file 0x3E9544：

```
0x3E953C  0x0C50F5FD   open
0x3E9540  0x0C50F615   close
0x3E9544  0x0C50F63D   ioctl（消息分派）
0x3E954C  "spi"        ← 紧跟设备名；同族记录里 0x3E94C4 是 "i2c"，条距 0x44
```
（这是 PSRAM 初始化数据段，VA = file-0x2FDC4+0x3C000000 → 0x3C3B9778。）

同一次搜索还送来一条**方法教训**：唯一"引用 0x3C3B9778 的地方"查到的是 file 0x44F810，
`fn` 一看那是一条 `str r7,[sp,#0x1e0]` —— **纯字节搜没有代码/数据门，把代码字节当成了引用**。
"引用点 N 处"这类结论一律要拿分析器的 xref 复核后才算数（纪律 16/17 的同一件事，换个方向再犯）。

**命令表（把 `tbb [pc,r3]` 的 16 项解出来，表基 0x0044F662）**：

| 消息 cmd | 目标 | 语义 |
|---|---|---|
| 0xF0 | `0x008A8A10` | 未查 |
| 0xFB | `0x008A89CC` | **帧请求**（只挂标志，不搬帧） |
| 0xFC | `0x008A8A30` | 未查 |
| **0xFD** | `0x008A8A64` | **搬帧**（8.61/8.62 一直盯的那个 worker） |
| 0xFF | `0x008A8A20` | 未查 |
| 其余 0x05/0x06/0x15/0x39 | 写/读引擎 | 走 0x0044F6A0 / 0x0044F6DA |
| 0xF1..0xFA、0xFE | — | 返回 `-ENOSYS`（`mvn r5,#0x25`） |

分派函数自己两道门：`[r0+0xC]==0` 直接返回 `-EBUSY`（`mvn r5,#0xf`）；`r1` 才是消息，
`[1]`=命令、`[2]`=标志、`[4]`=长度、`[8]`=数据指针。

**关键一条**：`xref 0x8A8A64` 在全镜像**只有 1 处引用**，就是上面 0xFD 那一档的
`0x0044F736 bl 0x008a8a64`。⇒ **搬帧的唯一入口是"有人对这台设备发 0xFD 消息"**；
8.62 写的"0xFB 档里 bl 帧请求 + bl 搬帧"不对，0xFB 只请求。渲染器那边也印证了：
`0x00890556 bl 0x008a89cc` 之后立刻 `pop …,pc`（只挂标志就返回）。

**帧请求 `0x008A89CC` 的完整动作**（这段是新的，之前只当它写标志）：

```
mrs basepri / msr basepri,#0x80          ; 临界区
if (*(u8*)(obj+0x150) >= 2) 直接出        ; obj = 0x3C3CA330
FUN_007FEF50(0xb, 7, …)
FUN_008A84B0(3, 1)                        ; ← 拉起显示引擎的那一跳
obj+0x154 ++ ; obj+0x148 = 1
msr basepri, 原值
```

`FUN_008A84B0(r0=位号, r1=置/清)` 逐字读：

```
008A84C2  ldr r3,[0x008a8500]  = 0x3C48E798   ; 模式位图
008A84C4  ldrb r4,[r3]           ; 现值
008A84CE  ldrb r0,[r6]           ; r6 = 0x3C48E799「已起动」标志
008A84D0  cbz  r4,  …            ; 位图空了就不起动
008A84D4  cbnz r0,  …            ; 已起过就不重起
008A84D8  bl 0x008ADCF8(r1=2)    ; 起动引擎（0x8AD7E0 读 / 0x8ADA0C 写 同族）
008A84DE  strb #1,[0x3C48E799]
```

也就是说：**请求那一刻固件自己就会把引擎拉起来**，并不需要谁再发 0xFD。
那么"LCDC 一次没被写"只有三种可能：(1) 帧请求那次 `obj+0x150` 门没过；
(2) `0x3C48E798/799` 这两个全局在初始化时没建立（`0x8A8500` 的池字本身是对的，但
位图内容取决于驱动 init 有没有跑过）；(3) `0x008ADCF8` 走到了但写的寄存器不是我们
认定的 `+0x224`。下一枪就打在这三个读数上：站点 `0x0C8A84B0 / 0x0C8ADCF8 / 0x0C8ADD68`，
停机时把 `0x3C48E790..0x3C48E7A0` 与 `0x3C3CA330+0x148` 一起落盘。

**现象记录（待判）**：`--wtrace 3C3CA4EC 4 40 0` 那一枪跑到 2.4G 之后进程直接结束、
没有收尾报告（退出码 1，日志里没有 `[FAIL]`）。同一份 profile 不带 `--wtrace` 时
2.0G 一切正常。先把这条当"观察窗本身可能是肇事者"的疑点记着，未定性。

### 8.65 第 24 轮：抓到"帧请求确实拉起了模式位"——257.7M 那一刻的现场（`--wstop`）

`--wstop 3C48E798 4 12000000`（跳过 crt0 清 .bss 那一遍）一枪命中：

```
[wstop] 第 257731185 条命中写：3C48E798 <- 00000008 (宽1) PC=0C9684CE LR=0C9689F3
```

- PC=0x0C9684CE → file 0x008A84CE = `FUN_008A84B0` 里 `strb r4,[r3]` 之后那一拍 —— 模式位图写入者就是它；
- LR=0x0C9689F3 → file 0x008A89F2 = `bl 0x008A84B0` 的返回地址（`0x8A89EE+5`），调用者是**帧请求**；
- 值 0x08 = 第 3 位置起 ⇒ 请求那一刻固件把"显示模式位 3"点上了。

**并且时刻是 257,731,185** —— 与 show logo 那一枪（257.7M）同一拍。也就是说 8.61/8.62 里
"帧请求命中 1 次 @2,441,173,959"只是**第二次**请求（8.64 的表里 `obj+0x154=2` 正是两次），
第一枪在 logo 那一拍。

栈顶现场（同一拍，SP=3C5C3390）里有 `3C3CA330`（显示对象）、`0C95055B`（渲染器
`0x00890556 bl 0x008a89cc` 的返回地址）、`0C950825`、`0C950B2F`、`0C504A6B`（日志包装），
还有几串 ASCII 残字（"load_nor"、"data rev…erse"）。残字只当候选，不当调用链（8.55 的规矩）。

于是"LCDC 为什么没起动"只剩两种可能，各配一枪：

1. 模式位图写上之后 `bl 0x008ADCF8(2)`（引擎起动）到底跑没跑 —— 站点
   `0x0C96DCF8`（换算过：file 0x008ADCF8），窗开在 257.0M 之后；同一枪带上
   `0x0C96DD68`（位图空时那条反查）与 `0x0C968A64`（搬帧）、`0x0C50F63C`（分派）。
2. `0x3C48E799`（"已起动"标志）有没有被写 —— `--wstop 3C48E799 1 250000000`。

**同时记两条我自己造成的取证口径错误（纪律 18）**：

- 站点地址**手敲**而不是从 `addr_conv.py` 的输出粘贴：`--site 0C8A89CC` 那批全是
  file 偏移没加 0x0C0C0000（应为 `0C9689CC`），于是一整枪静默零命中。这是"换算手滑"
  这一类伪因的第三次复发，而且复发在已经写了防护工具的轮次里 —— 工具在场不等于在用。
  规矩：挂点前每一条地址必须走 `addr_conv to`，把打印出来的那一行原样贴进命令行。
- `--site-after` 开窗位置选错：按 8.61 的"2.44G 那一次请求"把窗开在 2.3G，
  结果错过了**第一次**请求（257.7M）。窗口要按"最早的可能时刻"开，
  或者先用 `--wstop`（它天然抓第一笔）确定时代，再决定站点窗。

### 8.66 第 24 轮：257.7M 那一拍的完整次序（站点 + 第二道写断点）——引擎登记跑了，搬帧仍然一次没有

`--site-after 257000000` 配四个站点（地址全部现算现贴：`0C96DCF8/0C968A64/0C50F63C/0C96DD68`），
再单开一枪 `--wstop 3C48E799 1 250000000`。两边合起来把这一拍钉死：

```
257,731,185  3C48E798 <- 00000008 (宽1)  PC=0C9684CE LR=0C9689F3   ← 帧请求调模式置位器，位 3 点上
257,731,190  [site 0C96DCF8] lr=0C9684DD pc=0C96DCF8  r0=0 r1=2      ← FUN_008ADCF8(0,2) 引擎登记跑到
257,731,206  3C48E799 <- 00000001 (宽1)  PC=0C9684DE LR=0C9684DD   ← "已起动"标志由置位器自己写
```

⇒ **"请求那一刻固件自己会拉起引擎"这句成立**：`FUN_008A84B0 → FUN_008ADCF8(0,2) → 置 0x3C48E799=1`
整条链在模拟器里跑通了，一步不缺。

但同一窗口里三个站点**零命中**：`0x0C968A64`（搬帧）、`0x0C50F63C`（消息分派）、
`0x0C96DD68`（位图空时那条反查）。而且 `FUN_008ADCF8` 的函数体读完就知道它**不碰 LCDC**：

```
008ADCF8  ... 若"已起动字节"和"模式字"都为 0：置 1、bl 0x7FC718、
             r4 = DAT_008add60 + r0*0x20、把返回值写 [r4+0x12]、
             bl 0x7FEDB8 / 0x8AEAF0 / 0x8AEBE0
008ADD46  [DAT_008add5c] |= (1<<r1)          ← 只是把模式位或进一个状态字
```

也就是说：**p65 这套"帧请求"只做登记（标志 + 模式位 + 起动记账），出帧必须等 0xFD 那条消息**，
而这条消息在当前模拟里从未有人发出。这与 8.62 的判定一致，只是现在连"请求会顺手起动引擎"
这条可能性也被逐指令读掉了。

现场寄存器（257,731,190 那一拍）还留着一串可对照的数：`r3=3C48E798 r4=00000008 r5=00000080
r6=3C48E799 r7=3C5C33B0 r9=00000066 r12=20081740` —— `r9=0x66` 是帧请求传下来的第一个参数，
`0x66` 是不是"层号/面号"待定，别当结论用。

**下一跳只能是"谁该发 0xFD"**（技能里出帧闸第 5 道的那条：请求挂着没人搬）。已排除的候选：
渲染器只调请求就返回（`0x00890556`）、模式置位器不碰 LCDC、搬帧的唯一引用点是分派表 0xFD 档。
剩下的查法按便宜排序：

1. `--rtrace 3C3CA478 4 60 250000000` —— 看**谁读** obj+0x148 这个请求位。读的人里若出现
   搬帧以外的函数，那条就是真正的消费者；若只有 crt0/搬帧，就证明确实没人搬。
2. 拿 043 做对照：同一份 LCDC 模型在 043 上出过帧（`--panel` 有图），把 043 的"发 flush 的那一跳"
   找出来，再回 p65 找同名结构 —— 两个目标同源，比对形状比空想快。

### 8.67 第 24 轮：判据本身有问题 —— p65 的 LCDC 驱动里**根本没有写 +0x224 的代码**

先把一条可能"永远达不成"的判据挑出来，否则后面全是白跑。

**证据一（分析器，不是手搓）**：把 p65 里所有拿着 LCDC 基址的地方先定位 —— 全镜像按 4 字节小端搜
`0x40200000` 得 48 处，其中 **39 处集中在 file 0x8B1D00~0x8B2900**（这就是 p65 的 LCDC IP 驱动族），
另有 0x8A897C、0x894854、0x449041、0x25xxxx、0x1AAF52 几处散点。
再对 `0x8B0000~0x8B3200` 整段出反汇编，grep `0x224`：**零处**。这一段里与"启动"最像的只有

```
008B1EF0  str.w r0,[r3,#0x220]        008B2880/88/98/A2/…  ldr/str [r3,#0x228]
008B1204  mov.w r1,#0x220             （通用寄存器写口的偏移实参）
```

⇒ **`frame_start=+0x224` 是 best1503 那条线上的名字，p65 的驱动代码里不存在这条写**。
`docs/…profile` 侧确认：`profiles/best1502_r0.soc.json` 的 `devices.lcdc.regs` 与 `irq_bits`
是 **逐字照抄 best1503 的**（0xF0/0xF4/0xFC/0x104/0x1C0/0x1C4/0x224/0x264/0x280/0x284/0x230/0x140，
位 18/21/27），没有一条为 p65 核过。这按纪律 14 就是要避免的事：**硬件参数不许从兄弟目标平移**。

**证据二（阳性对照，说明判据在 043 上确实成立）**：同一份 LCDC 模型跑 043，
`--wstop 40200224 4 0` 一枪命中：

```
[wstop] 第 500001290 条命中写：40200224 <- 00000001 (宽4) PC=0C232370 LR=0C2261C7
```

`fn 0x172370`（043 工程）就是一条两行的小写口 `*(u32*)(base+0x224)=value`。
⇒ **模型没坏、判据在 043 上可达**；p65 达不到是因为 p65 的驱动不写那一格。

**证据三（p65 到底开了哪一位）**：把"Enabling the display"这句话的引用点反查回来 ——
字符串在 file 0x30866F，其 PSRAM 池字 VA = 0x3C2D88AB，全镜像两处字面量：
file **0x8A82FC** 与 **0x8A8410**，都在显示核 `FUN_008A8308` 这一族里。该函数（215M 跑过）依次调：

```
0x8ADDA4(0,2)  0x8B27A8  0x8B27D8(1)      ← LCDC IP 侧
   …打日志若干…（0x8A8392 处打 "Enabling the display"）
0x8A8414 取 obj → strd 0,[obj+0x154] → bl 0x41E684
0x8B26CC(x)   0x8B26EC(1)                 ← 关键
0x7FEF50(0xB,0)   0x8AD694(...)
```

`fn 0x8B26EC` 全文只干一件事：

```
if (DAT_008b270c) { r2 = [LCDC+0x1C0]; r0? → r2 |= (1<<23) : r2 &= ~(1<<23); [LCDC+0x1C0] = r2 }
```

⇒ **p65 的 LCDC 中断使能里用的是 bit23**，而我们的 `irq_bits` 只有 18/21/27 ——
既没有帧完成位的正确编号，也没有状态格的正确偏移（043 上驱动会读 `+0x1C4` 并写 1 清 ——
p65 在 3.2G 内**一次都没碰过 +0x1C4**，见 run B 的设备表）。

**结论与改法（顺序按性价比）**：

1. **先找 p65 的 LCDC 中断服务函数**：向量表在 SRAM `0x20166000`（110 槽），3.2G 落一份
   （`--ramdump 20166000 1C0`），看哪几槽不是自环跳板、其中哪一槽落在 0x8B1D00~0x8B2900 段里 ——
   那一槽就是 p65 真正的 LCDC 中断号（我们现在按 1503 用 **67**，很可能是错的）。
2. 读了 ISR 就知道它测哪一位、清哪一格 ⇒ 把 `irq_status`/`irq_bits` 按 p65 实测改，
   模型出帧的触发格（`frame_start`）也按驱动代码里真实存在的那一条写来定（候选：`+0x220`、`+0x210`）。
3. 这一步不改动 043/o66 的 profile 值（板级/目标级各留各的），改完必须跑全量 `build.sh` 回归。

**顺带一条取证事实**：`--rtrace 3C3CA478 4 60 250000000`（请求位 obj+0x148 的读者）在 3.2G 里
**一条读都没有** ⇒ 除了没跑的搬帧函数，确实没有别的消费者。这条要在 run K 里用
`--rtrace 3C48E798`（模式位图，搬帧函数一定读它）做阳性对照，否则"零"不算数。

### 8.68 第 24 轮：撤销"FUN_008A8A64 是搬帧消费者"；显示对象带一张 vtable，出帧口在那张表里

**撤销（纪律 18）**：8.61/8.62/8.67 一直把 `FUN_008A8A64` 叫"搬帧 worker/消费者"。
把它整个读完了 —— **它一个寄存器都不碰**。全文只有两次等待：

```
008A8A64  if (*DAT_008a8ad8 == 0) return                      ; 全局使能字节
          FUN_0040725C(obj+0x160, 0)                          ; sem 初始化
          obj+0x15d = 1 ; obj+0x179 = 1                        ; 临界区里置"在处理"
          while (obj+0x148 >= 1) { if (FUN_00406DE0(obj+0x160, 0xAA) != 0) break; }
          do { FUN_00406DE0(DAT_008a8ae0, (obj+0x140*17000+99)/100) } while (obj+0x140 > 0)
          obj+0x15d = 0
```

⇒ 它是**"等这一帧画完/等到 vsync"的阻塞函数**（两个 `FUN_00406DE0` = sem timedwait，
超时分别 0xAA 与 `obj+0x140` 毫秒），也就是 NuttX fb 那一侧的 `waitvsync` 形状。
所以"没人搬帧"这句不准确：**准确的说是"提交动作找不到、等待动作也没人调"**。

**消息各档语义（这次全读出来了，都是 `bl` 一条就返回）**：

| cmd | 目标 | 干什么 |
|---|---|---|
| 0xF0 | `0x008A8A10` | `obj+0x179 = 1` |
| 0xFB | `0x008A89CC` | 帧请求（标志 + 模式位 + 引擎登记） |
| 0xFC | `0x008A8A30` | `obj+0x150==0` 时 `bl 0x008ADDA4(0,2)` |
| 0xFD | `0x008A8A64` | **等帧完成**（上面那段） |
| 0xFF | `0x008A8A20` | `obj+0x179 = 0` |

**新抓手：显示对象自带一张 ops 表**。对象 `0x3C3CA330` 的**初值**就在镜像 PSRAM 初始化段
（file 0x3FA0F4 = VA 0x3C3CA330），把那块逐字读出来：

```
obj+0x00 = 0x3C48E764      （指向模式位图旁边的一格）
obj+0x04 = 0x0C967DA1      file 0x8A7DA0
obj+0x08 = 0x0C9677ED      file 0x8A77EC
obj+0x14 = 0x0C96845D      file 0x8A845C   ← 开机上电序列：bl 0x7FEB60/0x7FD980/0x8A8098/0x8A8440
obj+0x18 = 0x0C967D5D      file 0x8A7D5C
obj+0x1C = 0x0C967CF9      file 0x8A7CF8
obj+0x20 = 0x0C967E71      file 0x8A7E70   ← 读 [arg+0x31] 分档、越界报 0x8A7EEC 那条错、返回 -ENOSYS
```

这一族正是 NuttX `struct fb_vtable_s` 的形状（getvideo / getplane / setplane / getcolor /
setcolor / update / getdefault / setpower / waitvsync）。**`update` 才是"把这一帧交出去"的那一格**，
读它就能回答"p65 用什么寄存器起动扫描"，不用再猜 +0x224。

**两条辅助读数**：

1. 向量表（run K，3.2G 落盘 448 字节 @0x20166000，112 槽）：77 槽是默认跳板 `0x0C0C0085`，
   **31 槽（含 IRQ5、IRQ42、IRQ44、IRQ67…）全指向同一个入口桩 `0x0C50DAD9`**
   （file 0x44DAD8：`mrs ipsr` → `bl 0x00454560` = irq_dispatch）。
   ⇒ 真 ISR 在内核软件 irq 表里，**向量表看不出 LCDC 的中断号**；要按号找 LCDC 处理程序，
   得读 `0x00454560` 用的那张表（下一枪）。顺带：这解释了为什么 IRQ67 那一槽看着"像有人接"。
2. **rtrace 阳性对照成立**：`--rtrace 3C48E798 1 20 250000000` 打出两条读
   （257,731,179 与 2,441,174,017，PC=0x0C9684C4 = 模式置位器里那句 `ldrb r4,[r3]`）
   ⇒ 上一枪 `--rtrace 3C3CA478`（请求位 obj+0x148 的读者）零命中是**真零**，不是探针坏了。

**于是第 24 轮把问题重新表述成三句**：

1. p65 的出帧起动格不是 +0x224（驱动族里没有这条写，池里也没有 0x40200224 这个地址），
   所以 `lcdc 帧>0` 这个判据当前**按 1503 的口径不可能成立** —— 先把 `update` 那一格读出来，
   再决定模型该在哪一拍出帧（候选：layer enable 之后 free-run、或某个 +0x21x 位）。
2. 帧完成的通知方（`obj+0x160` 的 post）与 LCDC 真中断号仍未定 —— 读 `0x00454560` 的软件 irq 表。
3. 提交/等待这两条消息（0xFB/0xFD）在 3.2G 里一次都没被上层发 —— 这条要等 1 和 2 定下来
   才知道是"没人发"还是"发了也没用"。

### 8.69 第 24 轮：改用 MiBandStudio 的引擎做权威对照 —— 它自带 best1502p 机型，并且直接暴露我方 DSI 模型的缺口

用户点名不要自造轮子。核实结果：**MiBandStudio 0.0.1 的引擎就是 bes-qemu**
（`release/build-info.json`：repository `0x504B0304/bes-qemu`、commit `e368a0cc60fe830fb752772c150dc4ee3a949fc2`、
`config_abi: 2`），`release/catalog.json` 里**第一个设备就是 `redmi-watch6`（REDMI Watch 6）**，
而引擎的机型表里有 **`miband-best1502p`**（`qemu/machine/best1502p.c`，1299 行，公开可读）。
把它的源码取到本地 `ref/bes-qemu-1502p/` 逐段读，下面这些**不用再靠反汇编推**。

**已核的口径（作者源码为准）**：

| 东西 | 他们的值 | 我方现状 |
|---|---|---|
| LCDC 中断号 | **67**（`miband_lcdc_connect_irq(..., 67)`） | profile 用 67 ⇒ **对了**，不是抄错 |
| LCDC 出帧 | 写 `+0x224` 的 bit0 触发；60Hz 一个 `frame_tick` 定时器 | 一致（我方也是 0x224 驱动） |
| 帧完成位 | `TXC_FRAME_DONE=bit21`、`SCANOUT_DONE=bit27`、`CMD_DONE=bit18`；状态格 `+0x1C4` | 一致 |
| 图层格 | 源地址 `+0xF4`、pitch `+0xFC`、size `+0x104`、config `+0x264`（`[11:8]` 是格式） | 一致 |
| GPU | **VG-Lite 在 0x40220000、中断号 70**，`miband_gpu_connect_lcdc`，还有 `gpu-scanout-bridge`/`gpu-render-index8` 开关 | **我方完全没建 GPU** |
| DSI 窗 | `0x40230000`，用 **BEST1700 的 DSI 模型**（`qemu/soc/best1700/controllers/dsi/best1700_dsi.c`） | 我方自建 |
| DSI 寄存器 | `RX_STATUS=0x10`、`COMMAND=0x14`、`TX_HEADER=0x18`、`TX_WORD=0x1C`、`TX_FINAL1..4=0x20/24/28/2C`，`MMIO_SIZE=0x1000` | 0x10/0x14/0x18 **对上了** |
| DSI 包型 | `0x37` 设最大返回包长、`0x06` DCS 读（要 bus turnaround）、`0x05/0x15` 短写、`0x39` 长写 | 对上 |
| **DSI 事件位** | `RX_STATUS`：`CMD_DONE=bit0`、`BTA_DONE=bit1`、`BTA_FAIL=bit2`、`RX_DONE=bit3`、`RX_ERROR=bit4`、`TIMEOUT=bit6`、**`TE=bit7`**；`RX_READ_ENABLE=bit15` | **★我方缺这一整组**：dsi.c 读 `+0x10` 时把低 13 位掩掉、也从不置任何事件位 |

**这一条是本轮最值钱的发现**：我方 DSI 模型从不回 `CMD_DONE`/`BTA_DONE`/`RX_DONE`，
而驱动的写/读引擎按这些位判"命令已完成 / 收发结束"（读命令还要 bus turnaround 完成位）。
⇒ **"面板命令发了但没有下一步"、"搬帧/等帧消息一条没发"这两件事，很可能都卡在
`+0x10` 的事件位永远读不到** 上，而不是固件流程本身缺一步。这与 8.56 修 TE 后"命令仍未走完"
的现象是同一类根因（模型应答面不全）。先按这张表把 DSI 的事件位补齐，再重跑看
`+0x224` 与 `0xFD/0xFB` 是否自己动起来。

**怎么跑他们的引擎（记下来，别再试错）**：

- `hardware-config` 的 **`board` 段在 best1502p 机型上是坏的**：通用装配器
  `qemu/machine/bes_runtime_config.c` 无条件 `object_property_set_str(obj,"board-profile",...)`，
  而 `best1502p.c` 只注册了 `firmware-profile` ⇒ 报
  `Property 'miband-best1502p-machine.board-profile' not found`（文件式和 stdin 式都一样）。
  **绕法：不要用 hardware-config，直接把板级参数当 `-M` 机型属性给**（这些属性确实存在）：
  `display-width/display-height/panel-id/display-te-pin/touch-chip-id/touch-address/touch-i2c-bus/touch-irq-pin/nand-page-size/nand-spare-size/nand-pages-per-block/nand-page-count/nand-id/charger-model/charger-i2c-bus/charger-device-id/charger-probe-revision/boot-sp/boot-pc/flash-offset/resource-image/flash-state`。
- `-kernel` 的文件名**必须叫 `vela_ap.bin`**（否则 `miband-best1502p requires -kernel vela_ap.bin`）。
- 跑通到打印的一条命令（已实测能出真机开机横幅）：

```
engine/qemu-system-arm.exe -M "miband-best1502p,display-width=432,display-height=514,\
panel-id=3473696,display-te-pin=59,touch-chip-id=37399,touch-address=90,touch-i2c-bus=6,\
touch-irq-pin=29,nand-page-size=2048,nand-spare-size=64,nand-pages-per-block=64,\
nand-page-count=262144,nand-id=0xC8F4C8,charger-model=true,charger-i2c-bus=0,\
charger-device-id=7,charger-probe-revision=7,boot-sp=0x2019FFF0,boot-pc=0x0C0C0011,\
flash-offset=786432,resource-image=<绝对路径>/vela_resource.bin,flash-state=<可写路径>" \
 -kernel <绝对路径>/vela_ap.bin -display none -monitor none -serial file:uart.log \
 -d unimp -D qemu.log
```

**他们跑到哪儿**：AP 真机横幅全出（`CHIP=best1502p / KERNEL=NUTTX / BUILD_DATE=Sep 12 2026 07:22:16 /
REV_INFO=d516af0:nx_best1502p_ap`、`bes_chip_init_early done:1`、`RPTUN_INIT_DECLARE 616 0xc950a8b`），
**然后 117 次复位**，复位点全在 `RPTUN_INIT_DECLARE` 之后那一步 —— 即**核间（sensor/CP）起不来**：
NOR 阵列只由 `-kernel` 的 AP 在 `flash-offset` 处铺出来，第二颗 NOR 里的 `vela_sensor.bin` 没有入口
（机型表里没有 sensor-image 这一属性）。这正是我方 flint 已经解决过的一道闸（我方是
"挂 flash2 控制器 + 把 sensor 镜像铺进 0x22000000 阵列"）。
所以两个工具各有各的进度：**他们赢在器件模型面（GPU/DSI 事件位/panel/Zinitix/SC7061/NAND/ISPI-RTC 都是现成权威的），
我方赢在引导链（已经推到 3.2G 条、UI 起任务、触摸出 /dev/input0）**。
正确用法是**把他们当硬件参数的权威源与"另一双眼睛"，把缺的器件面抄过来**，
而不是在 flint 里继续凭反汇编猜寄存器。

**顺带纠正本轮早先的一处过推（纪律 18）**：8.67 写"p65 驱动里根本没有 +0x224 的写、
所以 `lcdc 帧>0` 按 1503 口径不可能成立"。当时只扫了 LCDC IP 族（file 0x8B0000~0x8B3200）
与显示核一段；现在按他们的源码，LCDC 出帧确实要写 0x224 —— 所以要么固件用**通用寄存器写口**
（`mov.w r1,#0x220` 那一族，偏移是运行期实参）间接写它，要么 `0x224` 这一跳被前置条件挡在门外。
**结论降级为待查**，别再当定案用。

### 8.70 第 24 轮落地：DSI 按权威模型补全（事件位/包布局/DCS 读应答）——跑完证明这不是拦路的那一道

做了三件事，全部按 MiBandStudio 引擎（bes-qemu `best1700_dsi.c` / `miband_panel_device.c`）抄齐，
不是自己发明：

1. **RX_STATUS(+0x10) 的事件位搬回 [23:16] 并"写 1 清"**；弹字节加 `RX_READ_ENABLE(bit15)` 门；
   影子只留 `value & ((0xFF<<24)|(7<<13))`。旧模型读 `+0x10` 时把低 13 位掩掉、
   事件位从不置起 —— 按 DCS 规范这就是"命令永不完成"。
2. **应答包换成权威 FIFO 布局**：`[0]=0x87` 转义字节、`[1]=DT|(包头&0xC0)`、`[2..3]` 长度、
   `[4]` ECC（六掩码奇偶）、载荷、再两字节 **CRC16**（反射多项式 0x8408），长度 = 载荷 + 7。
   旧模型是 5 字节头 + 载荷、ECC/CRC 位置填 0。
3. **DCS 读命令交给面板状态机答**。这条来自一个新的读数 —— 给模型加了"读命令直方图"之后，
   3.2G 跑出来的事实是 p65 驱动总共只问三条：

```
dsi 读命令记账（3 种）： 04x1 0Ax1 0Ex1
```

   `0x04` 取面板 ID（答 DDB，一直在答），`0x0A` 取电源模式、`0x0E` 取信号模式 ——
   后两条旧模型一律回零，按 DCS 规范等于说"屏还在睡眠、显示没开、TE 没开"。
   现在 `FUN_008B26EC` 那台面板状态机（`te_enabled/sleeping/display_on` 三个 bool）
   经 `fl_lcdc_panel_reply()` 真答出来：`0x0A → 0x08|（醒 0x10）|（显示开 0x04）`、
   `0x0E → TE 开 0x80`。

**结果（要如实记）**：改完之后 3.2G 跑与改之前**逐项相同** ——

| 读数 | 改前(run B/L) | 改后(run M) |
|---|---|---|
| `+0x10` 读/写 | 29/6（run B）→35/6 | 35/6 |
| `+0x14` 读/写、`+0x18` 写 | 30/22、21 | 30/22、21 |
| 面板命令条数 | 15（最后 0x51） | 15（最后 0x51） |
| `lcdc 帧` / `中断` | 0 / 0 | 0 / 0 |
| `te 给` / `gpio 中断` | 71 / 71 | 71 / 71 |
| 站点 搬帧(0xFD)/分派/screen_turn_on | 零命中 | 零命中 |

⇒ **DSI 应答面不是这道闸**（补它仍是对的：往后 GPU/面板真出帧时不能再欠这套位）。
这条假设到此否证，记下来免得再花一轮。

**当前定位（不变，且更硬）**：渲染器把请求挂上（`+0x148=1`、`+0x154=2`，257.7M 与 2.441G 各一次），
`FUN_008A84B0 → FUN_008ADCF8(0,2)` 引擎登记也跑了，但**没有任何客户端向这台设备发 0xFB/0xFD**，
`screen_turn_on` 也一次没进。下一枪在"发不出发"这一侧：按 2.45G 之后的**延迟日志全量收割**
看显示/UI 任务最后说过什么（run N），从它的台词定位它在等谁。

### 8.71 第 24 轮：2.456G 之后的运行台词只剩触摸 ESD 复位循环 —— 显示侧一句话都没有

`--site 0C504A54 400 --site-after 2450000000` 收 56 条，解串之后分成两段。

**第一段（2.4525G~2.4590G，屏幕服务与连接层，全 lv6 正常级）**：
`aod_mode/wrist_tilt_mode/touch_double_click/auto_brightness/brightness_value/dimmer_timeout`
一串设置项读取 → `g_screen.touchpad_dev_fd=%d` → `succeeded, tag=%s, handle=%p` →
四条 `tag/handle/status` → `tag/handle/auto_stop` → `succeeded to subscribe wrist_tilt` →
`## wrist_tilt_alarm_time …` → `alarm_id/type/timeout` → `keepon_enabled/duration` →
**`turn on screen, tag=%s`（2.4562G）** → `tag:%s, normal_stay_count:%d` 两条 →
`g_screen.screen_status_topic_fd=%d` → `[miconnect] start xpc client success`（2.4590G）。

**第二段（2.4848G 直到 3.1365G 上限，只有这个）**：

```
2484833823 lv3 [HYN] ESD data NA,need retry: 0x%x %d      ← 四条
2486204876 lv7 [HYN] esd_check power reset ic pre
2486248098 lv2 I2C info "close id %d"
2486304672 lv2 I2C info "open id %d, mode %d, dma %d, sync %d, speed %d"
2501921387 lv7 [HYN] esd_check power reset ic
   …2.80G 再来一遍、3.12G 再来一遍…
```

⇒ 两个结论：

1. **显示侧从 2.456G 起一句话没有。** 渲染器最后一次帧请求在 2.4412G；之后 740M 条指令里
   UI/显示/连接层没有任何台词，也没有任何错误。这与"分派函数 0xFB/0xFD 零命中"是同一件事的
   两种说法：**显示客户端停在了自己主循环里，不再往下走**（不是在报错、也不是在被别的东西拦）。
   下一步只能问"它卡在哪一次等待" —— 手上可打的判据点：`FUN_00406DE0`（nxsem timedwait）的
   等待者是谁、等的格子是哪一格（`obj+0x160` 之外还有 `DAT_008a8ae0`）。
2. **触摸 ESD 检查在反复失败并电源复位 IC**，每约 300M 条一轮。原因就是我方 CST92xx 端点
   没答 ESD 那组寄存器（引擎里 `board-devices/touch/cst9217/cst9217.c` 有权威实现，
   我方 `ref/bes-qemu-1502p/` 还没取这个文件）。这条循环本身不吃显示，但它在反复
   关/开 i2c 与复位触摸 IC —— 先按权威模型把它补对，省得它一直污染运行台词与 i2c 读数。
   （`cst9217.c` 已在 8.74 取回并逐条比对，结论是"权威模型里也没有 ESD 那组寄存器"，
   上一条的归因不成立，见 8.74。）

### 8.72 第 25 轮：显示客户端确实拿到了 fb —— `open("/dev/fb0")` 返回 fd 19

先记一句自我更正：**上一轮排的两个候选里，"fb 设备节点不存在 / open 静默失败"这一条已被实测否掉**。

静态一侧（Ghidra `xref` + `fn`，全部按 file 偏移查）：

- PSRAM 串池里 `/dev/fb0` 的字面量 VA = `0x3C1AC81C`（file 0x1DC5E0），全镜像 6 处字面量引用；
  其中一处 `file 0x9A62E8` 被 `FUN_009A6274` 在 `0x9A6298` 用 `ldr r0,[0x009a62e8]` 取走。
  这一处就在屏幕服务那个模块里（`0x9A54xx~0x9A63xx` 一整片，与 `g_screen=0x3C496800` 相邻）。
- `FUN_009A6274` 反编译出来是一段**标准的 fb 客户端初始化**，没有任何错误分支：

```
puVar1 = DAT_009a62e4;                        ← 全局块 0x3C4965B8（0x248 字节）
thunk_FUN_003663ac(puVar1, 0, 0x248, ...);    ← memset 整块
puVar1[7] = EXT_FUN_1c1580e4("/dev/fb0", 0x403);   ← open，返回值直接写进 块+0x1C
FUN_00569dd0(param_1, puVar1 + 0x42);         ← 两块描述符（+0x108 / +0x1A8）
FUN_00569dd0(param_1, puVar1 + 0x6A);
```

  `str r3,[r4,#0x1c]` 在 `0x9A62B6`，**前面没有 cmp、后面没有分支** —— 所以"打开失败会静默"
  这个形状是真的，但它是不是真发生了，只能实测。

动态一侧（run O，`--site 0CA662B6 4 --site-after 2300000000`，深 3.2G）：

```
[site 0CA662B6] 第 2453193491 条  pc=0CA662B6 lr=0CA662AF
   r0=3C5C1370  r1=3C4966C0  r2=00000001  r3=00000013  r4=3C4965B8  …
```

`r3 = 0x13 = 19` 就是要写进块里的那个返回值 ⇒ **fd 有效，fb 设备节点在模拟里是存在的**，
VFS、`register_framebuffer` 那条链、LCDC 驱动初始化全都走通了。这条候选到此关闭。

顺带把 `--ramdump 3C4965B8 248` 落的那 584 字节读了：

```
+000 = 3C5C1370   ← 传进来的 this（与站点 r0 同值）
+004 = 00000001
+01C = 00000013   ← fb fd = 19，一直保留到 3.2G
+0EC = 00000258   ← 600，另一处尺寸（未定含义）
+104 = 0000000D   +10C = 3C496770   +110 = 3C496D08
+130..+160 / +1D0..+200 = 一列代码域地址（0C514172、0C4C69A8、…、0C4C91A8）
                       其中 0CA662CE / 0CA662D8 正是本函数两条 bl 之后的返回地址
                       ⇒ 这两块是"调用现场记录"形状，不是 ops 表（含义待核）
```

**由此把问题收窄成一句**：客户端手上有有效 fd，但从 2.4532G 之后**再没有人读这一格**
（run P 的判据就是这个"再没人读"是不是真的）。如果 `+0x1C` 在 2.45G 之后零读者，
那"没人发 0xFB/0xFD"就不是分派表的问题，而是**这个客户端根本没走到用它的那一步** ——
它卡在 `open` 之后、第一次 flush 之前，卡点是等待而不是错误。

### 8.73 第 25 轮：按权威模型把 LCDC 那几个格的口径钉住（+0x280 是面板命令口，不是使能格）

`ref/bes-qemu-1502p/miband_lcdc.c`（引擎的 1502p LCDC 模型）读下来，把第 23~24 轮悬着的
三个命名问题一次对齐：

| 偏移 | 权威模型里的语义 | 出处 |
|---|---|---|
| `+0x1C0` | `LCDC_IRQ_ENABLE`，写它时立刻按 `status & enable` 拉中断线 | miband_lcdc.c:1119 |
| `+0x1C4` | `LCDC_IRQ_STATUS`，**固件写 0 清 bit18（命令完成锁存）** | :1123 |
| `+0x224` | `LCDC_FRAME_START`，**写值 bit0=1 就是"请求一帧"** | :1131 |
| `+0x280` | **面板命令触发口**：命令字取自 `(regs[0x284]>>8)&0xFF`，读标志取 `regs[0x230]&0x40`，读回的值落 `regs[0x140]` | :1149 |
| `+0xF4` | `LCDC_GRA_PITCH`（与 0xFC/0x104/0x264 一起算"配置已变"） | :880 |
| 完成位 | `bit18 = CMD_DONE`、`bit21 = TXC_FRAME_DONE`、`bit27 = SCANOUT_DONE` | :12 |

三条要紧的话：

1. **"固件打 `Enabling the display` 时 +0x280 没被写 ⇒ 使能格是别的格"这个推理作废** ——
   `+0x280` 从来不是使能格，它是 LCDC 内部那个**面板命令通道**（走 `panel.command()` 回调）。
   p65 的固件用 DSI 主机（0x40230000）发命令、不走这个口，所以它没被写是**正常现象**，
   不构成缺口。第 23 轮那条"真正的使能/启动写的是 +0x21C/+0x220/+0x228，命名待重核"
   连同这一条一起收回：**权威模型里 0x21C/0x220/0x228 没有任何特殊语义**，就是普通寄存器格。
2. **`+0x224` bit0 仍是唯一的"出帧请求"判据**（权威模型据此才 `frame_requested = true`）。
   p65 固件里找不到写它的代码这条（第 24 轮两次扫 Immediate 都没命中）保持"待查"，
   但**不再当成模型的缺口**：p65 的搬帧走 `FUN_0044F63C` 的 0xFD 档，那一条自己会去编程
   LCDC —— 只要有人发那条消息。所以顺序是先解决"谁发消息"，再回头看 +0x224。
3. **连参考实现自己都标了不确定**：`LCDC_SCANOUT_DONE (1u<<27)` 上面那句注释写的是
   "Layer-0's register group matches the observed GRA group (bit 27); the exact hardware
   bit still needs a real trace" —— 也就是**bit27 在权威源里也只是推断**。
   我方 profile/代码里凡按 bit27 判"帧完成"的地方，等级只能跟着标"推断待核"，不许写成实测。

### 8.74 第 25 轮：读追踪先做阳性对照，对照过了才敢下"没人读 fd"这句话

第 24 轮末尾我差点直接引用 run P 的"0x3C4965D4（fb fd 那一格）在 2.453G 之后零读者"。
这条是阴性结论，按纪律必须先用一个**必然会命中的对照**证明探针本身在跑，不然等于没测。

对照做法（run R）：拿同一个窗口口径去盯**这一轮已确认在反复读**的那一格 ——
定时器模块控制块 `0x3C3B98CC + 0xC = 0x3C3B98D8`：

```
[rtrace] 盯 3C3B98D8..3C3B98DB 的读，最多 5 条（第 2453000000 条之后才打）
  [rtrace] 第 2453000296 条 3C3B98D8 -> 00000000 (宽4) PC=0C514D1C
  [rtrace] 第 2453000696 条 3C3B98D8 -> 00000000 (宽4) PC=0C514D1C   ← 每 400 条一次
```

⇒ 探针在 RAM 侧确实出数，**run P 的零读者是可信的阴性**。但结论要说全：
这条只证明"没人在这段时间读 `块+0x1C`"，**不能**证明"没人用这个 fd" ——
fd 可能在 open 当时就被抄进别处（`0x3C5C1370` 那个对象里），读的是副本。
所以"显示客户端没发 flush"仍然只是观察，不是定案。

### 8.75 第 25 轮：真正在跑的是 `/dev/timer%d` 那条定时器链 —— 顺着它挖出了时基错误

尾段热点不再是一条自环，而是一族集中在 `0x0C9518C4~0x0C951A1A`（file 0x8918C4 起）的地址。
`fn 0x8918C4` 解出来是个**64 位有符号除法 helper**（除数为 0 时返回 `{-1,-1}`，慢路在
`FUN_00891988`）；它的调用者里有一族 PC 在 `0x0C514Dxx`（file 0x454Dxx），
站点现读把这条链的形状拿全了：

```
块 0x3C3B98CC = { +0: 0x3C182F44（一张函数指针表）,  +4: 0x3E80 = 16000,  +8: 0x0C4C66AD }
每次循环：blx r5 → 拿返回值；把 [r7+4]（实测 937 与 1000 两个值交替）× 16000 再 ÷ 1000000
调用点现场：lr=0C514D19 / 0C514D3F，r2 交替 0x1F40 与 0，sp=20173448
```

`0x3C182F44` 那张表按 PSRAM 口径解回 file `0x1B2D08`，紧邻的就是字符串
`armv8-m/default`、`/dev/timer%d`，后面跟 5 个代码指针（`0C514D85 / 0C514D6D / 0C514BC5 /
0C514C4D / 0C514C95`）与一个 `0C514CB9` —— **这是 NuttX 的 `/dev/timer%d` 上半部 ops 表**，
也就是 POSIX 定时器那条链。⇒ 尾段真正在跑的是**16 ms 一档的周期定时器**，
而设备窗口热点表里 `+010/+014/+018`（值 `0x00010003`、`0x10`、`0`）被读写六百到七百万次，
基址是 `0xE000E000` —— **那是 SysTick 的 CTRL / LOAD / VAL**。

顺着 LOAD=16 这条读数核时基，才挖出下面这个错（重要，直接改变所有等待/延时的语义）：

| 项 | 我方原值 | 权威值 | 出处 |
|---|---|---|---|
| `best1502_r0.soc.json` 的 `clocks.refclk_hz` | 1000000 | **16000** | `ref/bes-qemu-1502p/best1502p.c:1149-1151`（`clock_set_hz(s->refclk, 16000)`） |
| `clocks.sysclk_hz` | 100000000 | 100000000 | 同文件 1147-1148（这半边是对的） |

**根因：这个 1000000 是从兄弟目标 best1503 平移来的** —— profile 里 `_provenance.clocks`
那一行原本写的就是 `miband.c:669-672 clock_set_hz`（那是 band10pro/1503 的机器文件，
它的 refclk 确实是 1MHz）。1502p 的 refclk 与之差 62.5 倍，正是纪律 14 禁的那类错。

固件侧两路独立印证 16kHz 才对：

1. 运行期实测 `SysTick LOAD = 16`、`CTRL = 0x00010003`（bit2 `CLKSOURCE = 0` ⇒ 走外部参考时钟）。
   16 格 ÷ 16000Hz = **1 ms**，与 NuttX 的 1ms 节拍正好对上；按 1MHz 则是 16µs 一拍（62500Hz），
   而 1502 机器文件的 sysclk=100MHz 下"处理器时钟走 1ms 节拍"应当是 LOAD≈100000，
   固件根本没写过那个量级 —— 只有"外部 16kHz 参考时钟"这条路能解释 LOAD=16。
2. 上面那个定时器模块自己的换算常量就是 **16000**（控制块 +4 实测 `0x3E80`、
   站点现读 `r2=0x1F40/0x3E80`），与 refclk 同值 —— 这就是 `ns ↔ 参考时钟格` 的折算。

按 1MHz 建的后果：`scb.c` 的 `st_ext_div = sysclk/refclk` 从 6250 错成 100，
一拍该占 17×6250 ≈ 106,250 条指令却只占 17×100 = 1,700 条 ⇒
**OS 时间基准快了 62.5 倍**，3.2G 条里挂起 SysTick 458 万次、评估 2151 万拍、
异常进入 388 万次 —— 大量真实工作被节拍服务挤掉，所有"等 16ms"之类的等待
在模拟里根本不是它本来的时长。

已落地：`clocks.refclk_hz` 改 16000，`_provenance.clocks` 换成 1502p 出处并写清两路印证；
`scb.c` 的外部参考时钟那条路不用动（机制早就在，`test_scb` 的 T1b 就是断言它，
值是从 profile 现读的，所以改 profile 就够）。全量 `bash flint-vm/build.sh` 重跑 **rc=0**，
12 个单测与全部 sabotage 对照组都过（`test_scb` 对照组仍成立）。

**教训（要记进方法论）**：硬件参数"值能用"不等于"出处对"。我方此前因为
`clock.*` 有值、节拍也在走、引导也过了，就再没回查过这一格 ——
而它恰恰是唯一一处从兄弟目标平移来的时钟常量。核法应当是"每个时钟常量都要能指到
本机型（不是同族机型）的那一行 + 一个固件侧独立读数"，缺一律标 待核。

### 8.76 第 25 轮：时基修好之后，尾段不再是"节拍风暴"，而是一条有名有姓的忙等延时

同一口径（`--deep 3.2G --beat 400M`）修前修后对比，先看异常计数这一条硬指标：

| 采样点 | 修前（refclk=1MHz） | 修后（refclk=16kHz） |
|---|---|---|
| 400M 条 | 异常 进=355,748 返=355,747 | 异常 进=5,323 返=5,323 |
| 800M 条 | 进=517,788 | 进=5,467 |
| 3.2G 条（末态） | 进=3,886,888、SysTick 跳=21,510,525、挂起=4,586,326 | 进=176,229、挂起=1,454 |

⇒ 62.5 倍的节拍风暴确实是真实存在的负载，改掉之后同样 3.2G 条里系统做的"正事"完全换了样子：
**热点从"SysTick + /dev/timer%d 的换算"变成一族 SRAM 里的忙等地址**
（`0x0036FB94/96/98/9A` 四条，2.4G 起一直到 3.2G 末态都是它们，`LR=0x00390EB3`、`SP=3C762E10`）。

按 `addr_conv.py which` 反解（SRAM 执行别名 = file + `0x363AE8`，工具自己回代自证）：
`0x0036FB94 → file 0x0000C0AC`。Ghidra `fn 0xc0ac` 解出来的正是那条忙等：

```
FUN_0000c0a8(param_1):                      ; 入口 file 0xC0A8
   r2 = [0x0000c0b8]  = 0x40084000          ; 池字实测就是 AON_TIMER1 块基址
   r1 = [r2+4]                              ; 先读一次计数
loop: r3 = [r2+4]                            ; 再读
      r3 = r1 - r3                           ; 计数往下走 ⇒ 差值就是"已经过了多少格"
      cmp r0, r3 ; bhi loop                 ; 没等够就继续转
      bx lr
```

调用者散布在 `file 0x6374 / 0xAAA4 / 0xADF8 / 0xAE8E / 0xB00A / 0xB036 / 0xB042 / 0xAFEE…`
—— 全在**倒灌进 SRAM 的那段核管理代码**里（与 §"SRAM 内核代码 PC 走 0x0036xxxx 别名"同一段）。

**这条忙等等的对象我方模型是对的，不是缺设备**。按权威源逐格核对
（`ref/bes-qemu-1502p/best1502p_timer.c`，本轮新取回，两个下计数器按 0x20 跨距排）：

| 偏移 | 语义 | 位 |
|---|---|---|
| `+0x00` / `+0x18` | load | 装载值 |
| `+0x04` | **count（只读，往下计）** | 就是 `r1-[r2+4]` 读的那一格 |
| `+0x08` | control | `ONESHOT=0x01`、`WIDTH32=0x02`、`IRQ_ENABLE=0x20`、`PERIODIC=0x40`、`ENABLE=0x80`；`bit[3:2]` 是 16/256 分频 |
| `+0x0C` | 清 pending | |
| `+0x10` / `+0x14` | raw / 带 IRQ_ENABLE 掩码的状态 | |

并且 **既不是 ONESHOT 也不是 PERIODIC 时限值取 `WIDTH32 ? 0xFFFFFFFF : 0xFFFF`** ——
这就是那句"the AP delay helper reads its free-running downcounter"的机制。
`best1502p.c:1095-1098` 还给出这台块的两个通道频率：**ch0=16384Hz、ch1=16000Hz**
（我方 profile 里 aon_timer0=16384、aon_timer1=16000 与之一致 ✓）。

实测配置与计数都对得上（run T，`--wtrace 40084000 20 20 0` + `--rtrace 40084004 4 8 2400000000`）：

```
[wtrace] 第 94703 条 40084008 <- 00000082   PC=0C0C207E   ← ENABLE(0x80)|WIDTH32(0x02)，非周期 ⇒ 32 位自由往下计
[rtrace] 第 2400000002 条 40084004 -> FFFA000F (宽4) PC=0036FB94   ← 八次读数相隔 4 条指令
```

那八个读数一样**不是"计数冻住"**：一拍要占 `cpu_hz/tick_hz = 1e8/16384 = 6104` 条指令，
站点之间只差 4 条，本来就不该变。用同一个式子自检：2.4G 条应当走过
`2.4e9 × 16384 / 1e8 = 393,216` 格，而 `0xFFFFFFFF − 0xFFFA000F = 0x5FFF0 = 393,200` ✓ 对得上，
**我方 timer 模型的往下计数是对的**。

### 8.77 第 25 轮：时基修复把"跑到多少条指令"这件事的口径整个换掉了（重要）

新口径要记牢：**1 秒设备时间 = 1 亿条指令**（`clock.sysclk = 100MHz`、1 指令 1 周期）。
由此：

- `--deep 3.2G` 只等于 **32 秒**设备时间；`FUN_0000c0a8` 这种纯忙等延时按设备时间烧指令，
  尾段那一族地址很可能就是固件在**老老实实睡它该睡的时间**，不是死锁。
- 反过来看，修之前 SysTick 快 62.5 倍，等于"固件以为过了 1ms、实际只过了 17µs"——
  所有 `nxsem_timedwait`/超时/看门狗判据都在**假快**的时间里跑，等待几乎立刻到期，
  超时分支被虚假走进。**所以第 21~24 轮那些"跑到 2.4G 走到了哪一步"的结论，
  是建立在错误时基上的观察，需要在新时基下重取**（包括"0xFB/0xFD 分派零命中"这一条：
  它现在的含义是"32 秒设备时间内没人发"，而不是"整个启动过程没人发"）。
- 因此本轮末尾挂了一次 `--deep 16G`（约 160 秒设备时间）的长跑，判据仍是
  `lcdc 帧>0`、`--panel` 是否落 PNG、`0x0C50F63C` 是否终于被命中；
  结果单独记在下一节，不预测。

### 8.78 第 25 轮：新时基下的长跑结论 —— 约 6~7G 条之后是**硬停机**，停在一个终末自环

run U（`--deep 16G --beat 1G --site 0C50F63C 6 --uart-log uart1 --panel`）跑到 9G 之后我把它停了，
因为判据已经明确说"不动了"：

```
[beat] 3000000000 条 PC=0036FB9A LR=00390EB3 SP=3C762E10｜异常 进=147748 返=147748 pend=1434｜… gpio 中断=119｜UART1=3216
[beat] 7000000000 条 PC=0036EB36 LR=0036E5A7 SP=3C5BF2B0｜异常 进=447395 返=447395 pend=1631｜te 给=164 跳=4 gpio 中断=119
[beat] 8000000000 条 PC=0036EB36 LR=0036E5A7 SP=3C5BF2B0｜异常 进=447395 返=447395 pend=1631｜te 给=188 跳=4 gpio 中断=119
[beat] 9000000000 条 PC=0036EB36 LR=0036E5A7 SP=3C5BF2B0｜异常 进=447395 返=447395 pend=1631｜te 给=212 跳=4 gpio 中断=119
```

三条硬指标同时说"停了"：**异常进入/返回三个采样点完全相同**（447,395）、`gpio 中断` 也冻在 119、
只有我方自己发的 TE 沿还在涨（那是模型在走，不是固件在动）。
`lcdc 帧=0` 全程、`0x0C50F63C`（分派函数）零命中、`--panel` 没落文件。
按新口径换算：**跑到约 70 秒设备时间时固件停机**，不是"还在慢慢走"。

停点 `PC=0x0036EB36` 按 SRAM 执行别名反解 = **file 0x0000B04E**，Ghidra `block 0xb000 0xb080` 打出来是：

```
0000B030  bl 0x00006558
0000B034  movs r0,#0x50        ; 80
0000B036  bl 0x0000c0a8        ; 忙等延时 80 格
0000B03A  movs r4,#0xa         ; 10 次
0000B03C  bl 0x00006558
0000B040  movs r0,#0x10        ; 16
0000B042  bl 0x0000c0a8
0000B046  subs r4,#0x1
0000B048  bne 0x0000b040
0000B04A  bl 0x0000aa28
0000B04E  b 0x0000b04e         ; ★ 我们就冻在这里
```

同一族里 `0x0000B02E` 是同一个形状（`bl 0x0000aa28` 之后 `b .`），
而它前面是 `movw r0,#0x304f; bl 0x00006c9c; ldrh [sp+0xc]; lsls #0x1f; bpl …`
—— **读一个 16 位格、判它的最高位、不满足就再走一遍** 的等待。
⇒ 这一段不是"操作系统在闲等"，而是**核管理（PMU/时钟那族）代码里重试到尽头之后的自环收尾**：
重试 10 次、每次忙等 16 格，走完就叫 `0xaa28` 然后原地转圈。
这与第 23 轮那次"5.47G 冻死在 0x0036EB36"是**同一个停点**——当时把它当成"触摸没起来导致的
偶发"，实际上它是这条重试到尽头的路；新时基下它照旧出现（约 7G 条 = 70 秒设备时间）。

**缺口按权威源已经找到候选**：引擎为 1502p 建了一颗 **LPU（AON `WAKEUP_CLK_CFG`）**，
我方 profile 里**完全没有这一格**（`ref/bes-qemu-1502p/best1502p_lpu.c`，本轮取回）：

| 项 | 权威口径 |
|---|---|
| 位置 | 单格 4 字节，`0x4000008C` |
| `bit14` / `bit15` | osc 就绪 / pll 就绪（**只读，按到期时间自动置起**） |
| `bit12` / `bit13` | osc / pll 使能位 |
| `bit[7:0]` / `bit[11:8]` | 等待拍数（一拍 31250ns，标称 32kHz） |
| SDK 用法 | 注释写明"64 个振荡器等待拍 ≈ 2000us，7 个 PLL 拍 ≈ 200us" |
| 写行为 | 写进去时先 `&= ~(3<<14)` 把状态位滤掉；使能与拍数变了就重算到期时刻 |

上面那段"读 16 位、判最高位、重试到尽头"的形状，与 LPU 的"使能之后轮询就绪位"是对得上的；
但我方**不能就此定案**——那一格的基址与位号是引擎口径，还要先看固件自己在停机前说了什么
（run V 正在收 5.5G 之后的日志行），以及 `0x4000008C` 到底有没有被访问、读回什么。
这两条读数出来之前，LPU 只算"头号候选"。

### 8.79 第 25 轮：候选定案 —— `0x4000008C` 的读数把 LPU 钉死了，缺格已按权威源建好

上一节留的两问，答案是**从已跑的 run S/U 的设备窗口热点表里直接读出来的**，不必再跑一轮：

```
4000008C [mmio +008C] 读 838922 次  写 4 次  宽度 w  最近值=00001740  首/最近 PC=00369810
```

三条都对上权威口径（`ref/bes-qemu-1502p/best1502p_lpu.c`）：

| 实测 | 权威口径 | 判读 |
|---|---|---|
| 被读 **838,922** 次、只被写 4 次 | 就绪位只读、按到期时刻自动置起 | 这是"写完使能就轮询"的形状，不是配置扫描 |
| 写进去的值 = `0x1740` | bit12 = 振荡器使能；bit[7:0] = 等待拍数；bit[11:8] = PLL 拍数 | `0x1740` 拆出来是 **使能振荡器 + 等 64 拍 + PLL 段 7 拍** |
| 64 拍 / 7 拍这两个数 | 引擎注释原文："64 oscillator wait ticks for 2000 us and 7 PLL ticks for 200 us, nominal 32 kHz" | 固件用的就是 SDK 那一对典型值 |

⇒ **"读 16 位、判最高位、重试 10 次到尽头之后 `b .`" 那条路等的就是这一格**：
兜底锁存只会回读写进去的 `0x1740`，`bit14` 永不出现，固件就走到终末自环。
`0x304F` 那个 `movw` 不是地址而是寄存器号（它被 `bl 0x00006c9c` 当参数取走），
这一点与 LPU 的形状不冲突，但**它到底读的是哪一格仍未核**，记在这里不当结论用。

（run V 那轮日志收割我提前停了：它挂的窗口是 5.5G 之后，零命中——那段时间固件一次日志都没打，
而 `0x4000008C` 的访问证据已经从热点表拿到，这一轮的收获不在那 2G 条静默里。）

**已落地（按 AGENTS 的新建设备单元四步做满，`669d7b33`）**：

- `flint-vm/src/l2/dev/lpu.{c,h}`：两路共用一格的读写语义照权威源搬
  （写侧先 `&= ~(3<<ready)` 滤掉只读位；使能关掉就撤到期时刻；使能位或拍数字段一变就重算；
  拍数取 `MAX(t,1)` 不"当场就绪"；读侧按 `now_step` 与到期时刻比，到期才置就绪位）。
  **基址、位号、字段移位与宽度一律从 profile 读，代码里没有硬件地址**（CI 门照过）。
- `best1502_r0.soc.json`：区间表加 `lpu` 段（4 字节、嵌在 mmio 里，
  依据 `best1502p_lpu.c:72` 的 `add_subregion_overlap(..., 0x4000008c, ..., 3)`），
  `devices.lpu` 带 `tick_hz=32000`、`bit.enable=12`、`bit.ready=14`、两路 shift/width，
  出处同时写引擎行号与本轮那三条实测读数。
- `tests/unit/test_lpu.c`：39 项，**期望值全部用 profile 的两个频率现算、并且带余量**
  （半个时长不许就绪、满时长加一拍余量必须就绪），故意不断言实现里那个四舍五入的边界；
  含"只开一路时另一路不许就绪"、"改拍数要重算到期"、"就绪位写不进去"、
  "拍数 0 至少等一拍"、"窗口外访问要拒"这几条。`--sabotage` 反写四条评论性判据，退出码 0 ✓。
- `build.sh` 三处都加（`DEV`、单测名单、单独的对照组块 —— 它和 `test_dsi` 一样吃
  `profile_p65.prf`，1503 那份没有这一格）。
- 校验器（`flint_profile.py`）加 `lpu` 判据：就绪位必须在使能位之上、两路等待字段不许重叠、
  `tick_hz` 非 0、窗口尺寸必须是 4、有行为模型必须有同名窗口；
  并把**四种故意损坏**加进 `--selftest`，实跑都被大声拒绝 ✓（这一步是关键：
  设备单元最容易"建了但没人判它建得对不对"）。

全量 `bash flint-vm/build.sh` **rc=0**：`test_lpu` 39 项 0 失败、对照组成立，
其余 21 个单测与全部旧对照组不变。

**这一改动会不会把尾段放开，靠跑出来的读数说**（run X，`--deep 7.6G --beat 500M`，
判据仍是异常计数是否继续涨、`lcdc 帧>0`、`0x0C50F63C` 是否命中、`--panel` 是否落 PNG），
结果单独记下一节。

### 8.80 第 25 轮：LPU 接上之后硬停机没了——就绪位真的出现了，固件不再走到终末自环

run X（`--deep 7.6G --beat 500M`，与 run U 同口径）前后对比，只看硬指标：

| 读数 | 接 LPU 之前（run U） | 接 LPU 之后（run X） |
|---|---|---|
| 异常进入（末态） | **447,395，且 7G/8G/9G 三个采样点一模一样 ⇒ 停机** | **801,429，一路在涨**（5.5G=502,785 → 7.5G=787,513） |
| 停在哪 | PC 冻在 `0x0036EB36`（file 0x0000B04E 的 `bl 0xaa28; b .`） | 没有冻点；PC 在延时族 `0x0036FB94..9A` 里换 |
| `4000008C` 的写值 | 最近值 `0x00001740`（使能，就绪位从来不出现） | **最近值 `0x00005740`** ⇒ `bit14` 就绪位按到期时刻置起来了 |
| LPU 单元自述 | 不存在 | `lpu 读1623100 写4 宽度不认识0｜锁存=00001740｜两路到期还差(拍)=0 0` |
| gpio 中断 | 119（也冻住） | 一路涨（128 → 176） |

⇒ **尾段那条硬停机的根因确认并解掉了**：固件等的就是这一格的就绪位，
我方按权威口径把"使能之后按拍数到期自动置位"建出来，等待就能走完，
它也就不再走进"重试到尽头 + 原地转圈"那条收尾。
装配日志同时打得出这一格（`接上 LPU：4000008C（振荡器/PLL 两路，使能位 12、就绪位 14，标称 32000 Hz）`），
不是静默挂上去的。

还没达成的仍然是**唯一那条目标判据**：`lcdc 帧=0`、`--panel` 不落文件、
`0x0C50F63C`（该发 0xFB/0xFD 那条消息的分派入口）零命中。
`UART1` 字节数从 2.4G 起一直停在 3216 —— 这是延迟日志没被搬出去的老现象，不是"没话说"，
所以下一步（run Y）是把 2.4G 之后所有走日志包装函数的调用全收一遍，
看固件在这段时间到底说了什么、说到哪一句停住。

顺带记一个待核的数量级（不当结论用）：末态 `SysTick 跳 900,616 / 挂起 799,574`，
而按 `LOAD=16` 与 `refclk=16kHz` 应当是"每 100,000 条指令一次节拍"，
7.6G 条只该有约 76,000 次 —— 实测多了约 12 倍。
要么固件在运行期把 LOAD 改小过（要拿 `--wtrace E000E014` 现读才知道），
要么我方把"跳"与"挂起"两件事记成了同一个口径。**这两条都会改变"设备时间"的解释，先记下来再验。**

（这一条已经有答案了：**两个猜测都不对**。同一份报告里 NVIC 窗口自述
`E000E018(VAL) 被写 1,020,178 次`、`E000E010(CTRL) 写 1,693,539 次`，
而 `E000E014(LOAD) 只写 173,409 次` —— 固件把 SysTick 当**细粒度延时/时间戳计数器**在用，
每用一次就写 `VAL=0` 重装再使能，所以"挂起数≈重装次数"（799,574 对 1,020,178），
它本来就不该等于 OS 节拍数。我方按"写 0 之后下一拍到期"建的，与参考实现的 1 拍回卷一致，
所以这个数不是模型错。结论：**不许拿 SysTick 次数换算开机时长**，要换算就用 `LOAD=16 @16kHz`
那条推出来的 1 秒 = 1e8 条指令。）

### 8.81 第 25 轮：2.5G 到 7.6G 之间固件只说两类话，显示客户端一个字都没有

run Y 把日志包装函数（file 0x444A54，站点 `0C504A54`）从 2.4G 起全量收了一遍：
**命中 136 条**，窗口拉到 7.6G（= 约 76 秒设备时间），格式串按 dump 下来的串池
（`--ramdump 3C100000 300000`）逐条解回原文。分类只有两类：

```
① 触摸 ESD 那条循环，每约 317M 条（≈3.2 秒设备时间）一轮，一轮 7 句：
   lv3 [HYN][esd_check] ESD data NA,need retry: 0x%x %d     ×4
   lv7 [HYN][esd_check] esd_check power reset ic pre
   lv2 I2C info "close id %d"                              ← 实参 id=5
   lv2 I2C info "open id %d, mode %d, dma %d, sync %d, speed %d"
   lv7 [HYN][esd_check] esd_check power reset ic           ← 与上一句隔 16.5M 条 = 165ms
② 只有一句别的，在 6.911G：
   lv7 %s  wConsumeStatus = %d  erron %d tv_nsec = %ld     ← %s 解出 phOsalNfc_ConsumeSemaphore_WithTimeout
```

三条要紧的话：

1. **显示客户端从 2.45G 之后一次日志都没打过**（136 条里没有任何 UI/显示/连接层的行）。
   这跟"`0x0C50F63C` 零命中"、"`lcdc 帧=0`"是同一件事的三个说法：它不在跑，也没在报错。
2. **`esd_check` 那一条现在有名有姓**：第一个 `%s` 解出来是字符串 `esd_check`
   （不是先前猜的 worker 名），调用点在 file 0x4256DC / 0x425760 / 0x4257AC 三处。
   那句 `165ms` 的间隔是它自己做电源复位后的等待 —— 与新时基吻合（165ms = 16.5M 条），
   也说明**这条循环是准点跑的，不是我方的节拍把它拖慢的**。
3. 顺带出一个新缺口候选：`phOsalNfc_ConsumeSemaphore_WithTimeout` —— **NFC 的 OSAL 在等一个带超时的信号量**。
   这句只出现一次（69 秒设备时间），说明 NFC 线程也在等同一个不存在的对端（NFC 器件没建模）。
   它是不是首帧的上游，还没证据，先记成待核，不动模型。

另一件被这轮读数顺手改掉的事：`close id %d` 的实参是 **5**，而我方 profile 把
`0x40063000` 那颗控制器叫 `i2c6`。**这不矛盾**（驱动自己的端口枚举与我方按基址排的名字不是一套），
基址是先前用站点抓 `r4=0x40063000` 定下来的，仍然有效 ——
但两边名字不同这件事要写明，免得下一轮有人拿"id 5"去改 profile 的实例名。

### 8.82 第 25 轮：ESD 那条循环读到的是 `0x5A:[0xD048]`，判据逐条从固件代码里抄出来了

先补一句上一节的作废：那句"权威模型里也没有 ESD 那组寄存器，所以按权威模型补对这条路走不通"
**只说对了一半** —— 权威 `cst9217.c` 的应答表里确实没有 `D048`，但"没有"不等于"不用建"。
本轮把**判据本身**从固件里读出来了，所以这格该回什么是有依据的。

取 selector 的过程（不靠猜）：`--i2c-trace` 挂上之后端点自述给出
`端点：触摸(CST92xx) 地址90=0x5A selector=D048 mode=9 写102 读56`
—— 尾段最后一次登记的 selector 就是 **0xD048**。再拿 Ghidra 读调用它的那段（`fn 0x4256a0` 一带）：

```
0x00425692  movw r2,#0x48d0        ; 半字 0xD048（小端存进 r7 → 字节序 D0 48 = 高字节在前）
0x0042569A  strh r2,[r7]
0x0042569C  movs r0,#0x5a          ; 从地址 0x5A
0x0042569E  movs r2,#0x2           ; 2 字节
0x004256A0  blx r3                 ; 写 selector
0x004256B6  movs r0,#0x5a ; r2=2 ; blx r4      ; 读 2 字节回 r7
0x004256BA  cmp r0,#0 ; bne 另一支            ; 传输失败另走一支
0x004256C0  r3 = r7[0]                        ; 第一个字节
0x004256C2  r2 = r3 & 0x70 ; cmp r2,#0x20 ; beq 合法支     ← 判据一
0x00425712  r2 = r3 & 0x0F ; cmp r2,#9 ;  bhi 走 NA        ← 判据二
0x004256CA  …bl 0x00444a54                    ; "[HYN][esd_check] ESD data NA,need retry: 0x%x %d"
0x004256E2  subs r6,#1 ; uxtb ; cmp r6,#0xff ; beq 0x42574a ; 然后回到 0x425692 重来
```

站点读数与代码逐条对上：四次 NA 那一族的 **r6 = 3、2、1、0**（正是这个重试计数在往下走），
`r3 = 0`（读回来的第一个字节 = 0 ⇒ `0 & 0x70 = 0 ≠ 0x20` ⇒ 判不过 ⇒ NA），
`r0 = 3` 是日志级别 lv3，重试到 0 之后就走 `esd_check power reset ic`（其间 `2.5237G → 2.5393G`
隔 16.5M 条 = **165ms** 的复位等待，与新时基吻合）。

⇒ 我方端点对 `D048` 回 0 是**唯一**让它失败的原因，判据两条都写在上面。

已落地（`i2c6` 端点新增一格，值走 profile 不写死进源码）：

- profile `devices.i2c6` 加 `touch_esd_sel = 0xD048` 与 `touch_esd_value = 0x20`。
  **等级: 推断待核** —— `0x20` 只是满足固件判据（`(v&0x70)==0x20`、`(v&0x0F)<=9`）的最小值，
  真机这一格低 4 位大概还带状态码，本轮不求把它当已核事实，只求不再误判 NA。
- `src/l2/dev/i2c.{c,h}`：`cst_reg_word` 先查这一格（未配置就照旧回 0，行为不变）。
- `flint_profile.py`：两格**要么都给要么都不给**（只给号不给值等于悄悄回 0，
  正是"改了 profile 重跑读数一个字不变"那种最难查的形状）。
- `test_i2c_touch.c` 加 T6：**期望值抄固件的两条判据，不抄 profile 里那个值**
  （把值改坏，这一组必须变红），另验 selector 登记、高低字节序、第二字节回值。
- 全量 `bash flint-vm/build.sh` **rc=0**，端点自述已经出现
  `selector=D048`（说明 T6 真的走那一格），22 个单测与全部对照组照旧。

顺带这一轮的**待核清单**（都还没动模型，只是记下）：
`phOsalNfc_ConsumeSemaphore_WithTimeout`（NFC 的 OSAL 在等带超时的信号量，器件没建）；
`0x304F` 那个 `movw` 到底读的是哪一格（它是 `bl 0x00006c9c` 的参数，不是地址）；
`--site 0C4C6DE0`（`nxsem` 定时等待）在 2.5G 之后**零命中**这条还没做阳性对照，先不算结论。

### 8.83 第 25 轮：ESD 那一格补上之后，噪声确实没了——两个独立信号都对上

run BB（`--deep 3.2G --beat 400M --site 0C504A54 250 --site-after 2400000000`，修 ESD 之后）：

| 读数 | 修 ESD 之前 | 修 ESD 之后 |
|---|---|---|
| 日志包装函数在 2.4G→3.2G 的命中 | 约 14 条（全是 `[HYN][esd_check]` 那一族） | **0 条**（一次都没有） |
| `UART1` 字节数（400M/800M/1.6G/2.0G/2.4G） | 2850 / 2850 / 2926 / 3199 / 3216 | **逐点完全相同** 2850 / 2850 / 2926 / 3199 / 3216 |
| 触摸 IC 的电源复位循环 | 每约 317M 条一轮 | 不再出现 |

⇒ 两个信号一起说明同一件事：**改的只是那一格的回值，前面的时间线一个字没动，尾段那条误判循环消失了。**
这条很重要，因为"改了模型之后前面也变了"是最容易把修复说成副作用的情形；这里能明确分开。

顺带把**向量表**的实测读数记下来（`--ramdump 20166000 280`，500M 与 30M 各一次）：

```
槽 0（初始 SP）= 2019FFE0      槽 1（Reset）= 003952A5
槽 11 SVC      = 0C50DAD9  ← 与槽 15 同一个入口（file 0x50DAD8）
槽 15 SysTick  = 0C50DAD9
槽 14 PendSV   = 0C0C0085   ← 还是默认跳板（30M 时所有系统槽都是这个值）
槽 46 aon_timer1 = 0C0C0085
槽 67 LCDC       = 0C0C0085
```

三条判读要分开说：

1. **SVC 入口拿到了**（`0x0C50DAD9` → file 0x50DAD8，工具回代自证过），这一轮的 SVC 现场就挂在它上面；
2. **LCDC 与 aon_timer1 的中断向量在 500M 时还没被装**，这与 043 那条定案一致
   （"没有任何静态证据说 NVIC 67 被打开"），所以**帧完成走的是轮询不是中断**，
   我方那条 `lcdc 中断=0` 不是模型缺中断；
3. **PendSV 槽还是默认跳板**这一条要标"未定"：它可能意味着上下文切换这条路没建立，
   也可能只是这张表不是运行中的那张（要看固件写的 VTOR 到底指哪儿）。
   本轮先按"未定"记，不据此改任何东西 —— 判它在不在用，得看 SVC/PendSV 的实际进入次数，
   这正是下一节那次 SVC 现场取样要回答的。

### 8.84 第 25 轮：SVC 取样给我的是**我自己探针的结论**，不是固件的 —— 当场收回

run CC（`--site 0C50DAD8 60 --site-after 2400000000`，挂在向量表槽 11 那个入口上）两条读数：

```
VTOR = 20166000   MSP = 201734B0   CONTROL = 2   lr = 00390EB3   pc = 0036FB98
SVC 现场命中：2.4G→3.2G 之间 0 次（全跑下来 SVC 也才 846 次）
```

`VTOR=20166000` 先把上一节那条"这张表是不是运行中那张"的疑问关掉 ✓ 就是它。

但"SVC 零命中"这条我原本想用来判"没有任务在调内核"——**这个用法不成立，当场收回**：
这份固件是 flat 构建，`ioctl`/`read`/`nxsem_wait` 这些是直接函数调用，**不走 SVC 指令**；
所以"SVC 没被用"是**我方探针选错了入口**，不是固件的行为。
反证就在同一批读数里：`CONTROL=2` ⇒ 线程态用 PSP、栈在 PSRAM `0x3C762E10`，
而 6.911G 那句 `phOsalNfc_ConsumeSemaphore_WithTimeout` 是**另一个线程**打出来的
——线程切换在发生，多个线程确实拿到过 CPU。**所以"多线程死了"这个说法作废。**

由此把问题重新钉成一句（这才是下一步要测的）：
**显示客户端在 2.456G 打完 `turn on screen` 之后就不再有动作，而它手上有有效 fd=19；
按 `--site 0C4C6DE0`（nxsem 定时等待）在 2.5G 之后零命中这件事的正确读法是
"没有线程在 2.5G 之后**新进入**等待"，不是"没有线程在等"** ——
已经睡在某一个等待里的人不会再经过那个入口。**要判的是"谁在睡、睡在哪一格信号量"，
取证点应当是"在 2.5G 之前最后一次进入等待的那批现场"，或者直接把等待队列本身读出来。**

这一轮真正落得住的收获，按证据强度排：

1. 时基从兄弟目标平移来的 1MHz 改成本机型权威的 16kHz（异常进入 388 万 → 17.6 万）；
2. 尾段硬停机根因是缺 AON LPU 那一格，按四步建好之后异常计数一路在涨、终末自环不再进去；
3. 触摸 ESD 误判（`0x5A:[0xD048]` 回 0）补掉，前段时间线逐字节不变、尾段噪声消失；
4. `open("/dev/fb0")` 返回 fd=19，"fb 节点不存在"这条候选关闭；
5. LCDC 寄存器口径按权威源改正（`+0x280` 是面板命令口，不是使能格；`+0x224` bit0 仍是唯一出帧判据）；
6. `VTOR=0x20166000` 就是运行中的向量表；LCDC/aon_timer1/PendSV 三槽在 500M 时仍是默认跳板。

### 8.85 第 25 轮收口：挂起的帧请求位一次都没被人读过（"客户端在等帧完成"这条也关掉）

run DD（`--rtrace 3C3CA478 4 14 0`，起拍 0、拉到 3.2G 全程）：装上了盯
`3C3CA478..3C3CA47B` 读、最多 14 条，**之后一行都没有**。

`3C3CA478` 就是显示设备对象 `0x3C3CA330` 的 `+0x148`（渲染器写的那个"帧请求挂着"标志，
实测被写过两次）。全程零读者 ⇒

- 先前那条猜测「客户端发出帧请求之后睡在"等完成"上」**不成立**：连读这个标志的人都没有，
  更没有谁在它上面自旋等待；
- 与另两条独立读数拼起来是自洽的：搬帧 worker 等的信号量 `obj+0x160` 自 crt0 之后再无写入
  （没人等它）、分派函数 `0x0C50F63C` 零命中（没人发 0xFB/0xFD）；
- ⇒ **显示客户端不是"卡在驱动里"，它是根本不往下走了**：打完 `turn on screen`
  之后既不再调驱动、也不再读它自己挂上去的那个请求位。

下一轮的取证方向因此定死一条（记在任务 #18）：**别再用进入点取样**（看不见已经睡着的人，
见 `flint-runtime-evidence` 规则 4b），改从 `nxsem_post_impl` 已读到的等待队列形状
（节点 `[r3]`=next、`+0x18`=超时标志、`+0x1C`=pid）出发，**把信号量的等待链表读出来**，
直接回答"哪些任务在睡、睡在哪一格、那一格本该由谁 post"。
候选格：`obj+0x160 = 0x3C3CA490`、`DAT_008a8ae0`、显示设备对象 `0x3C3CA330` 一族、
面板结构 `0x3C3D54F8` 的 `+0x410..+0x412`（背光门仍为 0）。

### 8.86 第 25 轮：显示链上第一个**机制级**原因抓到了 —— 一个再也没被清掉的"已投递"闩

顺着"客户端不再往下走"从代码里找它的动作出口，链路是这样接上的：

```
屏幕服务 file 0x9A7674（FUN_009a6274 那一族的服务对象 = 字面量 0x3C496800 = g_screen）
   ├─ 读 g_screen+0x1BC（会话计数）自增
   ├─ 只有"自增后 ==1"那一次才走下一跳
   └─ FUN_00561CA0(g_screen + 0x8D0)      ← 投递"把状态应用到 UI/显示"的那一跳
FUN_00561CA0(对象):                        ; file 0x561CA0
   piVar3 = 对象 + 0x7C                    ; ★"已经投递过"闩
   if (*piVar3 != 0) return 0;             ; ★置着就直接返回，什么都不做
   对象[+0x18]++ ; *piVar3 = 1 ;
   fd = *(*(对象+4) + 0x16C) ; 用内核调用腿 0x1C159A2C 往这个 fd 写 1 字节
```

对象 = `g_screen + 0x8D0 = 0x3C4970D0`，闩格 = `+0x7C = 0x3C49714C`。
`--wtrace 3C49714C 4 24 0` 全程读数（run FF）：

```
第      11,595,181 条  3C49714C <- 00000000   PC=0C0C0354     ← crt0 清 .bss
第 2,114,328,078 条    3C49714C <- 00000000   PC=00366428     ← SRAM 段里的一次清
第 2,114,340,588 条    3C49714C <- 00000000   PC=0C621C2A     ← file 0x561C2A（消费侧的清零）
第 2,115,908,981 条    3C49714C <- 00000001   PC=0C621CC8     ← FUN_00561CA0 自己置 1
   ……之后再没有任何写入；停机那一刻 dump 出来 闩 = 1
```

**结论（都是实测）：**
- 投递**成功发生了一次**（2.1159G 置闩，且按代码那三支判据没走进 assert ⇒ 那一笔写进 fd 的调用返回成功），
  此后闩一直是 1；
- 消费侧的清零在 2.1143G 跑过（就在那次投递之前），**之后一次都没有**；
- 投递函数自己永不清闩（它只把 `+0x18` 的计数减回去），清闩是**消费者**的活；
- ⇒ **2.1159G 之后，所有"应用屏幕状态/投递显示"的动作都被这一格静默短路**，
  与"日志零条、分派 0xFB/0xFD 零命中、帧请求位零读者"三件事完全对得上——
  显示客户端不是卡在驱动里，是**它的下一跳被一个没人清的闩挡住了**。
- 顺带把 fd 拿到手：`--ramdump 3C7150F8 200` 读出句柄 `+0x16C = 9` ⇒ **消费者应当是睡在 fd 9 上的那个线程**
  （fd 9 早于 input0=13、fb0=19 打开，是 UI/OSAL 那条事件队列）。

**下一跳要答的问句已经收窄成一句**：谁在读 fd 9、它此刻在哪。
可打的点（按性价比排）：
1. 找 `read()` 在 fd 9 上的读者：把站点挂在文件系统那条读路径（不是分派函数）上，
   现读 fd 与 SP——SP 能区分线程（本项目已见过 `0x3C5BF498`/`0x3C762E10`/`0x2017xxxx` 三种栈）；
2. 读 fd 9 的 `struct file`（`f_oflags/f_inode/f_priv`）看它是不是 fifo/eventfd、有没有读者在等；
   NuttX 的等待队列形状可以从 `nxsem_post_impl` 那三格（`[r3]` next、`+0x18` 超时标志、`+0x1C` pid）拿；
3. 若确认"写成功但读者不在"，那就不是模拟缺口而是**该线程根本没起来或在别处睡死**——
   那时要问的是它的创建方（rcS 那条链在 p65 上是否跑到 UI 任务的创建）——
   注意这条与"闩在置起之前那次清零是谁做的"是同一条时间线，两边读数应当互相对得上。

★另外记一条与本问题无关但同批读到的形状：`E000E100(ISER0)` 与 `E000E280(ICPR0)`
各被写约 **84.7 万次**、值 `0x20`（IRQ5 的"使能/清挂起"成对出现），PC 在 file 0x8A30EA/0x8A30EE；
这是又一条自旋循环（与上一轮那条 705K 次的 AON 看门狗形状同一族），本轮没动它。

## 第 26 轮（2026-10-18）：尾段不是卡死 —— 那一轮只跑了 32 设备秒，其中 8 秒是一次有名有姓的忙等

### 8.87 先把"跨目标照抄"这一条收回（纪律 18）

上一轮排"首帧抓手"时写过一句：`p65 的 LCDC 驱动族在 file 0x8B1E00~0x8B2900，
与 043 的 0x171E00~0x172700 同形`。**这句作废。** 本轮拿两份镜像逐字节比：
`0x171E00` 与 `0x8B1E00` 起 0x900 字节窗口**只有 2.0% 相同**，
`0x171EDA` 与 `0x8B1EDA` 两段 64 字节完全不同。
真实情况只是"低 12 位数字撞上了"（0x…1EDA 对 0x…1EDA），我把它当成了模块同源。
⇒ 依据这条推断定的抓手（去比 043 的 `fn 0x172370` 与 p65 的 `0x8B2370`）**一并作废**；
p65 的 LCDC 侧只能按 p65 自己的代码与读数来定。

### 8.88 run JJ（3.2G 条）：LCDC 启动窗的写入者全捞到了，`+0x224` 依然一次没有

`--wtrace 4020021C 10 24 0`（盯 `+0x21C..+0x22B`）整轮只有 5 笔，全在 1.73 亿条那一次初始化里：

```
第 173,247,624 条 4020021C <- 05020020   PC=0C971EDA   (file 0x8B1EDA)
第 173,247,632 条 40200220 <- 001F001E   PC=0C971EF0   (file 0x8B1EF0)
第 173,256,259 条 40200228 <- 00000000   PC=0C972888   (file 0x8B2888)
第 173,256,266 条 40200228 <- 10000000   PC=0C9728A2
第 173,256,275 条 40200228 <- 10000000   PC=0C9728E0
第 173,256,278 条 40200228 <- 1A000000   PC=0C9728EA
```

`+0x224`（1503 那条线上的 frame_start）**一次都没被写** —— 这条与 8.67 的静态结论一致：
p65 驱动里没有这条写。另外两件事顺带钉住：`+0x228` 被写了 4 次且最后一值是 `0x1A000000`
（这一格我们目前没有任何 p65 口径的名字），`+0x1C4`（IRQ_STATUS）本轮仍然零碰。

### 8.89 投递那一笔 `write` 的返回值拿到了：**成功，而且成功两次**

站点挂在 `bl 0x007FE040` 之后那一条（file 0x561D02 = 运行地址 `0x0C621D02`，
`--site-after 2115000000`）：

```
[site 0C621D02] 第 2,115,588,557 条 sp=3C762D70｜r0=00000001 r1=00000000 r2=3C64E9D0 r3=00000001
                 r4=3C494A84 r5=00000009 r6=3C19D9EC r7=3C762D70
[site 0C621D02] 第 2,115,909,482 条 sp=3C762E28｜r0=00000001 r1=00000000 r2=3C64E9D0 r3=00000001
                 r4=3C4970E8 r5=00000009 r6=3C19D9EC r7=3C762E28
```

三条结论：

1. **r0=1 ⇒ 返回值是 1**（成功，且 `write` 返回的就是写入字节数）。所以 8.86 里那句"没走进断言"
   不只是没触发断言，是真的写成功了 —— 事件确实进了 fd 9 那一侧。
   代码自己就把这一读钉死了：`adds r3,r0,#1; beq 0x561CEE`（返回 -1 才去判 errno）、
   `cmp r0,#1; beq 0x561CD2`（返回 1 就走进"把 +0x18 计数减回去"的成功支）。
2. ★**站点在 `bl` 之后，所以只有 `r0` 是结论、`r1..r3` 不是入参**：现场 `r1=00000000`、
   `r2=3C64E9D0`、`r3=00000001` 是**被调方（PSRAM 里的内核 write）留下的残值**，
   拿它们反推"缓冲指针/长度"是过度解读（`r2` 是个 PSRAM 指针而不是长度就说明次序已被改过）。
   能跨调用保住的只有 callee-saved 那几个：`r4=对象+0x18`、**`r5=00000009` = fd 9**
   （代码在 0x561CE8 亲自把 fd 取进 r5 供重试循环用）。
   要取入参得把站点挂到 `bl` 那一条本身（`0x0C621CFE`），本轮没挂。
3. **投递发生了两次**，对象不同：`r4` 分别是 `0x3C494A84` 与 `0x3C4970E8`
   （= 对象 +0x18，即两个对象 `0x3C494A6C` 与 `0x3C4970D0`）。8.86 只看到后者那一次，
   因为那轮 wtrace 盯的是后者的闩格。两次都在 2.115~2.116G 之间。

### 8.90 顺带挖到的两条机制事实（对后面每个目标都用得上）

**(a) 内核不在 flash XIP，跑在 PSRAM 执行别名 `0x1C000000` 域。**
`FUN_00561CA0` 那个 `bl 0x007FE040` 解出来是 file 0x7FE000 起的一张 **8 字节一条的调用腿表**
（`bl.w` 形态的链接器长跳板 + 紧随其后的 32 位目标），本项目此前只在 SRAM 别名 `0x0036xxxx`
上见过这类腿。表头 16 条实测目标：

```
file 0x7FE000 -> 0037EFF5      0x7FE020 -> 1C128C01      0x7FE040 -> 1C159A2D  ← write（实测确认）
file 0x7FE008 -> 1C14885D      0x7FE028 -> 1C00800D      0x7FE048 -> 1C138965
file 0x7FE010 -> 1C020C45      0x7FE030 -> 1C008565      0x7FE050 -> 1C056E35
file 0x7FE018 -> 1C020C59      0x7FE038 -> 1C061A35      0x7FE058 -> 00369673
```

⇒ 以后要挂"libc 系统调用那一层"的站点，起点是这张表，不是猜一个 SVC 号。
**注意口径**：目标是 Thumb 指针（末位 1），站点地址要 `&~1` 并用 `addr_conv.py` 反解；
`0x1C…` 落在 PSRAM 执行别名域，Ghidra 工程（只装 flash）里没有对应代码，
想看实现得先按"影子源法"把 PSRAM 那段 dump 下来回搜。

**(b) `FUN_0044F63C` 不是 POSIX ioctl，是"带缓冲的消息分派"，形状读全了**：

```
r0 = 会话/设备对象：+0x0C 一字节为 0 就直接返回 -0x10；+0x10 起是 100 字节名字段
r1 = 消息结构：+0x00 一个方向位（bit2 判）、+0x01 cmd 字节、+0x04 长度、
              +0x08 缓冲指针、+0x0C/+0x10 两个 32 位参数
cmd >= 0xF0 ：tbb 五档，无参直调显示族函数
              0xF0->FUN_008A8A10  0xFB->FUN_008A89CC(帧请求)  0xFC->FUN_008A8A30
              0xFD->FUN_008A8A64(等帧/等 vsync，8.68 已定性)   0xFF->FUN_008A8A20
              其余一律返回 -0x26（ENOTTY）
cmd <  0xF0 ：5 / 6 / 0x15 / 0x39 四档带缓冲，走 FUN_008ADDA4 / FUN_008ADA0C / FUN_008AD7E0
              再用 FUN_00409F78(对象+0x10, 名字, 100, 0) 拷名字
```

全镜像按四个别名域搜 `0x0C44F63D`/`0x0C44F63C`/`0x2C44F63D`/`0x2844F63D` **一处字面量都没有**
⇒ 它的入口只能运行期填（这条与 p65"名字池无静态引用"同一族现象），
所以"谁发 0xFD"这个问题**静态问不出来**，只能动态等那一趟真发消息的调用者出现。

### 8.91 尾段定性改写：800M 条 = 一次 8.0 设备秒的忙等，跑的那条任务就是投递者本人

停机现场（run JJ，3.2G 条到上限）：

```
PC=0036FB98（= file 0xC0AC 那条 AON 忙等，8.76 定过性）
PSP=3C762E10  PSPLIM=3C743020  CONTROL=2  PRIMASK=0 BASEPRI=00  IPSR=0
r0=00020000  r1=FFF8B328  r2=40084000  r3=0000B319  r8=3C3CA1AC
异常 进入176178 返回176178 SVC846 挂起1438｜SysTick 跳196616
```

三件事一对就通了：

- `r0 = 0x20000 = 131072` 拍，AON ch0 = 16384 Hz ⇒ **等 8.000 设备秒**；
  1 拍占 `1e8/16384 = 6104` 条 ⇒ 8 秒正好 **800,025,600 条**，
  与"2.4G 之后到 3.2G 上限"那 8 亿条严丝合缝。
- `r3 = 0xB319 = 45849` 拍 = 已经走过的 2.80 秒 ⇒ 这一次忙等**还剩 5.2 秒**，
  也就是**还差约 5.2 亿条**才走到下一句。
- `PSP=3C762E10` 与投递那一刻的 `sp=3C762E28`/`3C762D70` 是**同一条栈**
  ⇒ 2.1159G 那两次投递、以及此刻这 8 秒忙等，**是同一条任务在做的事**；
  `PRIMASK=0`、`BASEPRI=00`、异常进出相等 ⇒ 它没有关中断、也没有把调度器锁住。
  ★所以"显示客户端被这条忙等饿死"这个猜测**不成立**：忙等期间系统照样在正常进出异常，
  尾段只是这一条任务在大面积 `sleep`。

⇒ **本轮定性（把上一轮的判读换掉）**：3.2G 条不是"卡在显示链某一道闸"，
而是**设备时间根本还没走完开机这一段**。上一轮把时基修对（refclk 62.5 倍）之后，
每一秒设备时间都要真烧一亿条指令，"跑到 3.2G"现在只等于"开机后 32 秒"；
而 rcS 那一串任务（`offline_log`/`log_service`/`kvdbd`/`chargerd`/`bluetoothd`/
`miwear_bluetooth`/`nfc_stack_bridge`/`miconnect`/`miwear`）本身就在做带秒级的启动等待。

**因此第 26 轮的第一枪改成：把设备时间跑够。** run KK =
`--deep 24000000000`（240 设备秒）`--beat 1000000000`，
站点同时挂三个（分派 `0x0C50F63C`、`screen_turn_on 0x0CA69200`、等帧 `0x0CA28A64`），
LCDC 启动窗 wtrace 继续盯，`--panel` 落 PNG，uart1 全量收日志。
判据不变且可分辨：**`lcdc 帧>0` / `+0x224` 或 p65 真正的启动格出现写入 / PNG 有内容 / 三个站点有没有命中**。
如果 240 秒里三个站点还是零命中，那"等够时间"这条就否证，回到"谁该发那条消息"上去查。

顺带记两条与本问题无关但同批读到的形状：`STCTRL(E000E010)` 被读 **3,452,448 次**
（最近 PC=`0C514CFC` = file 0x454CFC，SysTick 基延时助手在循环里被反复调）、
`ISER0`/`ICPR0` 各 81.9 万次值 `0x20`（IRQ5 使能/清挂起成对，file 0x8A30EA/0x8A30EE）——
这两条自旋形状本轮都没动。

### 8.92 run KK（`--deep 24G` = 240 设备秒）：**"等够时间"这条被否证**，尾段真相换成"固件在约 80 秒自己走收尾"

站点三挂（分派 `0x0C50F63C`、`screen_turn_on 0x0CA69200`、等帧 `0x0CA28A64`）**整轮零命中**，
`--panel` 没落 PNG，LCDC 启动窗那 6 笔写入与 3.2G 那轮**逐笔相同**（全在 1.73 亿条那次初始化里），
`+0x224` 依旧零写。⇒ 上一节"只是设备时间没到"这条**当场作废**（纪律 18）。

真正的时间线在 beat 表里（每行 `异常 进/返` 是唯一的"还在推进"信号）：

```
1G   PC=0C96E96C  SP=20173430   进=5,515
3G   PC=0036FB96  SP=3C762E10   进=147,697        ← 那条任务在 AON 忙等里
8G   PC=0036FB94  SP=3C762E10   进=858,007
9G   PC=0036EB36  SP=3C5BF2B0   进=873,872  ← ★从这一行起进/返/pend 全部冻住
24G  PC=0036EB36  SP=3C5BF2B0   进=873,872  pend=1,858   （te 给 还在涨是我方模型自己在发沿）
```

`873,872` 这个数从 9G 一直顶到 24G 一个都不涨 ⇒ **约 80~90 设备秒之间系统就不再干活了**，
后面 15G 条全在同一条自环里空转。停机现场 `PRIMASK=0` 但 **`BASEPRI=0x80`**（中断被按优先级挡住）、
`PC=0x0036EB36`、`编码=0000E7FE`（就是 `b .`）。

### 8.93 自环与它的入口读全了：这是一段 **PMU 收尾序列**的终点，不是断言转储

按 SRAM 执行别名换算（`file = 别名 - 0x363AE8`）：`0x0036EB36 → file 0xB04E`。
Ghidra `fn 0xB04E` 落进 `FUN_0000b030`，全文就是一份"发完最后几句话、等两拍、然后永转"：

```
FUN_0000b030:                       ; file 0xB030
  bl 0x00006558                      ; （flush 那一族）
  delay(0x50)                        ; bl 0xC0A8 = AON 拍忙等
  r4 = 10 ; bl 0x00006558
  do { delay(0x10) ; r4-- } while (r4)
  bl 0x0000aa28
  b .                                ; ★ file 0xB04E = 我们停机那一条
```

唯一的进入点是 `0x0000AF20` 那条 **`beq.w 0x0000b030`**，它紧跟在 `cmp r5,#0x0` 之后。
★`r5` 的来历本轮读死了：把窗口拉到全段（file 0xAC00..0xAF22，184 行）逐条找写 `r5` 的地方，
只有一处 —— **`0x0000AD1C push {r4,r5,lr}` 之后的 `0x0000AD1E mov r5,r0`**。
⇒ 这个大函数的真入口是 file 0xAD1C，**`r5` 就是它的第一个实参**，中间没人改过它；
`0xAF1E cmp r5,#0` 判的是"**被叫起来时那个参数是不是 0**"。
（我先写的是"r5 是更外层的 callee-saved 值"——那句不准，按这条改：不是外层变量，是**入参**。）
`0x0000AF24 ldr r5,[0xB060]` 是另一支（参数非 0 那一支）之后才重新装的，与这道判据无关。
★口径记一条：这类"整段里没有一处写某寄存器"的结论，窗口必须拉到**真入口**再逐条筛，
我第一遍只取了 `0xAD90..0xAF1E` 就下话，差点把"入参"读成"外层残留"。

这段所在的大函数是**一长串 PMU 寄存器读改写**（`bl 0x6C9C` 读 16 位、`bl 0x6D24` 写，r0 是 PMU 寄存器号），
实测到的寄存器号与动作：`0x3005 |= 0x100`、`0x303F &= 0x7FFF`、`0x311E |= 0x2000`、`0x3117 |= 0x1000`、
`0x3119 |= 0x400`、`0x304F |= 1`（**100 次一轮、每次 `delay(0x50)`**）、`0x3022 = 0x7FFE`，
末尾 `bl 0x8DA0` 之后就是那句 `cmp r5,#0`。
★顺带把上一轮那条待核答掉了：`movw r0,#0x304F` 那个参数是 **PMU 寄存器号**，
`FUN_00006C9C`/`FUN_00006D24` 是 PMU 的 16 位读/写包装（走 ISPI `0x40009000`，本轮实测传输 7,673 笔）——
**不是地址**。

⇒ **定性（把 8.91 那句"设备时间没到"补完）**：跑到 24G 也看不到首帧，是因为固件在 80~90 设备秒
走到了这段 PMU 收尾的终点并停在 `b .` 上 —— 系统在**关机/收尾**，不是在被什么闸挡着。
显示链那些"零命中"读数的解释也要跟着换：**不是客户端睡在某格信号量上，是整机已经不再推进**。

### 8.94 这条形状在本线出现过两次，都是同一族 —— 先记入口径避免第三次踩

1. 第 25 轮那条硬停机在 `0x0000B02E`（同一族的另一个自环），根因是 **AON LPU 就绪位**没握手（已建好、已解）；
2. 本轮 `0x0000B04E`，同一个大函数往上的一支，判据是 `r5 == 0`。
3. 另一条同源记录：043 侧 `pmu_shutdown` 的 `b .` 曾经挂在蓝牙驱动那句
   `'1503 A don't support BTC PD'` 断言上（CHIP_ID 预置那条链），当时"停在 pmu_shutdown 的 b ."这个现象
   与本轮一模一样。⇒ **看到 SRAM 里的 `b .` 先按"PMU 收尾/关机"这条线问，别按"操作系统闲等"读。**

下一枪（run MM，`--deep 9.2G` + `--site-after 7.8G`）已经在收尾段台词：
站点挂日志包装函数 `0x0C504A54`（预算 300）加收尾入口 `0x0036EB20`，
问的是"**关机是谁请求的、依据哪个读数**"。串口这边帮不上：UART1 从 2.5G 起就冻在 3216 字节
（延迟日志没人搬），只有挂包装函数才看得见。

### 8.95 run MM 把整条尾段问明白了：**是 `miwear_init.c` 里的显式 `PANIC()` 把机器打死的**

跑法：`--deep 9.2G --site 0C504A54 300 --site 0036EB20 300 --site-after 7.8G`
（站点挂在日志包装函数上收台词，配另一个站点想抓"走进收尾"那一刻）。
一次跑出 **84 条台词**，全部落在 `8,111,073,249 .. 8,114,306,374` 这 3.2M 条窗口里
—— 7.8G 到 8.111G 之间（3.1 设备秒）**固件一句话都没说**。台词按时间序（左边是第几条）：

```
8,111,073,249  lvl7  %s  wConsumeStatus = %d  erron %d tv_nsec = %ld        SP=3C6D1E58 lr=0C696D97
8,111,084,704  lvl3  SeApi: Error getting Secure Element info                SP=3C6D1E90 lr=0C68F749
8,111,093,128  lvl3  SeApi: NFCEE Initialize Failed status = %d                     lr=0C68F73F
8,111,115,789  lvl7  Exit TmlTask by condition                               SP=3C71BA50
8,111,127,432  lvl7  Exit TmlWriterTask by condition                          SP=3C71D668
8,112,475,266  lvl0  %s: Current Version: %s %s %s %s %s                      SP=3C5BF3C0 lr=0C4C33DB
8,112,484,229  lvl0  %s: Assertion failed %s: at file: %s:%d task: %s process: %s %p
8,112,496,443..  lvl0  R0..R12 / xPSR+BASEPRI+CONTROL / EXC_RETURN / Stack: base/size/sp
                       + 12 行栈窗 + 任务表表头 + 19 行任务表 + 27 行 backtrace
8,114,225,567  lvl0  Remote: %s headrx %u        ×2
8,114,240,281  lvl0  reset mode:%d
8,114,247,117  lvl0  Dump board_reset backtrace:  （再跟 16 行 backtrace）
8,114,293,089  lvl0  Version: %s %s %s %s %s
8,114,306,374  lvl0  Time: %s
```

**断言现场那一笔把话说死了**（站点现场 r0-r12 现读，串池口径 `file = VA - 0x3C000000 + 0x2FDC4` 解回）：

```
r0=00000000（level）  r1=3C1690E8 = "%s: Assertion failed %s: at file: %s:%d task: %s process: %s %p"
r2=3C169320 = "_assert"                    r3=3C1693BC = "panic"
r5=3C2AC2A8 = "/home/work/data/miui_codes/build_home_rom/apps/vendor/xiaomi/miwear/apps/system/miwear_init.c"
r6=3C1693BC = "panic"                       sp=3C5BF3C0  lr=0C4C3411
```

⇒ 表达式那一位就是字面量 **`panic`**：这是代码里显式写的 `PANIC()`，不是某个 `DEBUGASSERT` 的条件不成立；
**发生地 = `apps/vendor/xiaomi/miwear/apps/system/miwear_init.c`**。
这条链的形状与 043 那次一模一样（断言 → 转储寄存器/任务表/backtrace → `reset mode` → `board_reset`
→ SRAM 里 PMU 收尾 → `b .`），差别只是我方模型没有复位，所以**永远停在那条 `b .` 上**。

**这换掉了整条线的定性**：8.92/8.93 读到的"尾段 PMU 收尾 + `b .`"不是固件主动关机，
而是**panic 之后的收尾**；显示链所有"零命中"（分派、`screen_turn_on`、等帧消息）都必须重新解释——
系统在 81 设备秒就被打死了，根本没走到能画第一帧的那一段。

顺带把 PANIC 点也反查到了（**这次用字面量反查，一次就中**）：

- `0x3C2AC2A8`（那串文件路径）全镜像**只有 1 处引用 = file 0x86C2CA**（`ldr r2,[0x0086c2e8]`）；
- `0x3C169320`（`_assert`）与 `0x3C1690E8`（本条格式串）各只 1 处（file 0x40365C / 0x403664）
  = NuttX `_assert` 实现自己；
- `0x3C1693BC`（`panic`）14 处 —— 那是全镜像 14 个 `PANIC()` 档位的池字，需要逐条分辨。
- ★坑（同一个区域第二次撞）：Ghidra 把 `0x86C2E8` 那片**池字解成了代码**（`stmia r2!,{r3,r5,r7}`、
  `svc 0x3b`、`ldr r7,[r2,#0x68]` 那六条全是假的，`fn 0x86C2EC` 给的"函数"也是伪函数）。
  真口径是 `0x86C2CA` 那条 `ldr r2,[池]`。上一轮在 `0x86C1EC` 撞的是同一件事。

### 8.96 ★我自己这一枪有一发是哑弹，当场收回

`--site 0036EB20` 那个"走进收尾"的站点**零命中**，我差点据此写"收尾函数没被调用"。
按换算器现算才看清：`file 0xB030` 的 SRAM 执行别名是 **`0x0036EB18`**，
而我手工加出来的 `0x0036EB20` 解回 **file 0xB038 —— 那是 `bl 0xC0A8`（0xB036）的中间半字**，
根本不是指令边界 ⇒ 站点永远不会匹配，零命中是**探针挂错**，不是事实。
（反证本来就在手里：停机点 `0x0036EB36` = file 0xB04E 就在那条自环里，说明收尾确实跑到了。）
⇒ 纪律照旧：**站点地址一律由 `addr_conv.py to` 现算并粘贴，不许心算加法**；
`which` 那一侧要能把站点值解回我想要的那个 file 偏移，两边写同一行才算过。

### 8.98 run NP（把 panic 那一刻的栈窗落盘）：拿到设备时间、板名，还拿到"复位也没成功"

`--deep 9.2G --ramdump 3C5BF000 900 build/p65_besqemu/runNP_panicstack.bin`（一次跑只认一条 ramdump，
地址按 panic 任务现场 `sp=3C5BF3C0` 向下取整到 0x900 覆盖）。落出来的可读片段：

```
3C5BF09E  Z<12/31 00:01:20          ← 设备时钟：80 秒（与 8.11G 条 / 1e8 完全对得上）
3C5BF0F4  31 00:01:20] [ 1] [NOTICE] [ap]
3C5BF115  Error: pmu wdt reboot faile
3C5BF181  Error: pmu wdt reboot failed!     ← ★第二次
3C5BF322  Z<NuttX / REDMI Watch 6 FFFF / 0.0.0 / af8ec192 Sep 12 2026 07:22:22
3C5BF411  NuttX / REDMI Watch 6 FFFF ...    ← 转储头（板名自证是本机型）
```

⇒ 尾段的完整次序定下来了：**PANIC 转储 → `reset mode:%d` → 请求 PMU 看门狗复位 →
`Error: pmu wdt reboot failed!`（两遍）→ SRAM 里的 PMU 收尾 → `b .`**。
我方模型没有实现那条 PMU 看门狗复位，所以永远停在收尾自环里 ——
**真机在这里是重启（boot loop），模拟里是冻住**。这也解释了为什么 24G 那轮
`te 给` 还在涨（我方自己在发垂直同步沿）而系统读数一动不动。

### 8.99 PANIC 点整条读出来了：`miwear_init.c:187`，而且发 PANIC 的那一段是**自己给自己登记周期回调**

先按 8.95 的反查落点（file 0x86C2CA）把最后四条指令补齐 —— 这次不猜，逐条读：

```
0x86C2C8  movs r1,#0xbb                 ; 行号 = 0xBB = 187
0x86C2CA  ldr  r2,[0x86c2e8]            ; r2 = "/home/.../apps/system/miwear_init.c"（池在 0x86C2EC）
0x86C2CC  ldr  r0,[0x86c2ec]            ; r0 = "panic"（池在 0x86C2E8）
0x86C2CE  bl   0x004374e8               ; = _assert(expr, file, line) ⇒ miwear_init.c:187 的 PANIC()
```

`bl 0x4374E8` 这个包装在本线已经第二次出现（`FUN_00561CA0` 的投递重试耗尽那一支
`0x561D0E ldr r0,[=...]` + `movs r1,#0xd4` + `bl 0x4374e8` 就是同一个 `_assert`，行号 212）
⇒ **`0x0C4374E8(expr, file, line)` 是 p65 的断言入口**，两条都是实测/静态互证。

发 PANIC 的那个函数（file **0x86C204** 起，真入口 `push {r4..r10,lr}`）读下来的形状：

```
0x86C20C  bl 0x004021dc            ; 取一个 64 位量（r1:r0）—— 时间源，单位待核
0x86C210..0x86C286  一长串 64 位乘加移位（乘数池值 0x8D4FDF3B / 0x83126E96 / 0x83126E97）
0x86C282  ldr r4,[0x86c2d8]        ; r4 = 0x3C479B08（一个 64 位格，存"上一次/期限"）
0x86C28A  ldrd r6,r5,[r4]          ; 读回旧值
0x86C290  cmp r3,r6 / sbcs r2,r5
0x86C296  bcs 0x86c2b6             ; 新值 >= 旧值 ⇒ 去算差值
0x86C298  strd r3,r2,[r4]          ; 否则把新值存回去
0x86C2A0  ldr r4,[0x86c2dc]        ; 0x000493E0 = 300000
0x86C2A8  ldr r2,[0x86c2e0]        ; 0x0C92C205 = **本函数自己的入口**（Thumb 指针）
0x86C2AA  ldr r1,[0x86c2e4]        ; 0x3C479BA8（一个对象）
0x86C2AC  bl 0x0040a38c            ; (0, 对象, 回调=自己, 300000, 0) ⇒ 重新登记周期回调
0x86C2B0  返回
0x86C2B6  movw r1,#0x7531          ; 30001
0x86C2BA  subs r3,r3,r6 / sbc r2,r2,r5     ; delta = 新 - 旧
0x86C2C0  cmp r3,r1 / sbcs r2,r2,#0
0x86C2C6  bcc 0x86c29c             ; delta < 30001 ⇒ 回去重新登记
                     否则 ----->  0x86C2C8 那四条 ⇒ PANIC(miwear_init.c:187)
```

⇒ **机制级结论（这条替代 8.95 里"SE/NFCEE 失败导致 panic"的猜测——那两个是**别的任务**打的，
SP 分别是 `3C6D1E58`/`3C6D1E90`/`3C71BA50`/`3C71D668`，panic 那条任务的栈是 `3C5BF3xx`；
时间上只差 1.2 毫秒，相关不是因果）**：

`miwear_init.c:187` 那个 `PANIC()` 是一个**自我续期的周期回调里的"超时即炸"看门狗** ——
函数每次跑到这里就把自己再登记一遍（周期常量 300000），并且拿"当前 64 位时间"减"存在 0x3C479B08 的旧值"，
差值一旦 **≥ 30001** 就直接 PANIC。也就是说：**UI 刷新回调被拖晚了，整个系统被判死。**

★两条必须标清楚的待核（不许当已证）：
1. **时间源与单位**：`bl 0x004021DC` 返回的 64 位量是什么（`system_tic64`？µs？ns？），
   以及 300000/30001 的单位。这一条决定"差值 30001"到底是 30 ms、300 ms 还是 30 µs。
2. **`0x3C479B08` 那一格是谁先写的**：本函数只在"新值 < 旧值"时才更新它，
   那第一次写入（也就是"期限"从哪来）必然在别处 —— 用 `--wtrace 3C479B08` 现读。

按这条线，尾段的因果换成：**"刷新回调为什么会被拖晚 30001"**才是上游问题，
而 2.4G→8G 那段"同一条任务反复 8 秒 AON 忙等"正好就是把回调拖晚的那件事；
SE/NFCEE 那几条只是同时间窗内的别的任务在说话。

### 8.100 下一枪（run NQ）：把时间源与期限格一起拿
`--deep 9.2G --site 0C92C210 12 --site-after 7000000000 --wtrace 3C479B08 8 24 0`
站点挂在 `bl 0x4021dc` 之后那一条（file 0x86C210 → 运行地址 0x0C92C210）现读返回的 64 位时间；
`--wtrace` 盯 `0x3C479B08` 那 8 字节看"期限"由谁在什么时候写。
判据：读数能不能把 `delta >= 30001` 与"8 秒忙等"对上，以及时间源单位能不能被这两个读数夹出来。

### 8.101 run NQ 把"拖晚即炸"量出来了：阈值是 **30 秒**，而期限从 21.1 秒起就再没被推过

两条读数一起拿（`--deep 9.2G --site 0C92C210 12 --site-after 7000000000 --wtrace 3C479B08 8 24 0`）：

```
[wtrace] 第 11,474,793 条 3C479B08 <- 00000000  PC=0C0C0354      ← crt0 清 .bss（低半）
[wtrace] 第 11,474,797 条 3C479B0C <- 00000000  PC=0C0C0354      ← 高半
[wtrace] 第 2,112,429,658 条  3C479B08 <- 00005283 (高半 0)  PC=0C92C394   ★唯一一次业务写入
   ……之后 60 设备秒之内再无写入，一直到停机
[site 0C92C210] 第 8,112,473,670 条  r0=000C60E6 r1=00000000     ← bl 0x4021DC 的返回（64 位）
```

**单位先定住（这条把 8.99 里两条待核都答了）**：`FUN_004021DC` 里面是
`FUN_0041A534`，它取 `(秒, 纳秒)` 再拼 `秒*10000 + 纳秒/100000` ⇒ **返回值的单位 = 100 µs（1 万下/秒）**。
自检：`811,750 × 100µs = 81.175 秒`，与"第 8,112,473,670 条 / 1e8 = 81.12 设备秒"对得上（差 0.06%）✓

可看门狗比的不是这个原始值，它前面还有一段"×100 再按魔数 64 位除"的换算
（`0x86C210..0x86C286`，魔数池值 `0x8D4FDF3B / 0x83126E96 / 0x83126E97`）。
把两个独立读数摆在一起就能把换算后的单位夹出来：

| 事件 | 那一刻的设备秒 | 数值 | 若按 ms 解释 |
|---|---|---|---|
| `0x86C394` 写入期限格 | 21.124 秒 | 21,123 | 21,123 ms ✓ |
| PANIC 那一刻的当前值 | 81.12 秒 | 约 81,120 | 81,120 ms ✓ |
| 阈值 | | 30,001 | **30 秒** |

`81,120 - 21,123 = 59,997 >= 30,001` ⇒ 正好在这一拍炸掉 ⇒ **换算后的单位是 ms（等级：两个独立读数互证的推断待核，不是权威表）**，
登记周期那个 `0x493E0 = 300,000` 是给定时器接口的（若是 µs 就是 300 ms 周期，这条仍未证）。

★**更正 8.99 的口径**：我当时写"拖晚即炸"，读起来像几十毫秒级的小拖晚；实测阈值是 **30 秒**。
这条差别很重要，因为它把因果翻了个面：

- **PANIC 不是原因，是结果**。真正的事实是：`0x3C479B08` 这一格在 21.124 秒被
  `0x0C92C394`（file 0x86C394，同属 miwear_init 那族）写过一次之后，
  **整整 60 设备秒再没人推过它**；而看门狗回调每次跑到这里都会拿"当前 ms - 期限 ms"比 30 秒。
- 于是问题被收窄成一句，跟 8.91 的现场完全对上：**21.1 秒那一刻之后，谁把 UI 刷新链按住不动了** ——
  按住的就是同一条任务（`PSP=3C762E10`）在 2.4G→8G 之间反复走的 **8.000 秒 AON 忙等**
  （8 秒 × 约 7 轮 ≈ 56 秒，正好填上 21.1 秒到 81.1 秒这段空窗）。
- 而 8.100 记的"投递成功但消费侧不动"也在这同一时间窗里（2.115~2.116G = 21.15 秒）。

### 8.102 下一枪（run NS，已在跑）：把那条 8 秒忙等的**调用者**问出来

站点挂在 AON 忙等助手入口 `0x0036FB90`（file 0xC0A8，换算器现算并回代自证），
`--site-after 2500000000`、预算 10 —— 命中时 `r0` 就是等待拍数、`lr` 是调用者、`sp` 分线程。
这一枪要回答的是：**"每轮 8 秒"是谁在等、等的是什么**（是等帧完成、等对端核、还是等一个我方没给条件的器件）。
拿到 `lr` 之后按口径换算回 file 偏移读那个函数，不许拿别名反解当调用链（8.84 那类哑弹）。


## 第 26 轮后半（2026-10-18）：8.100 那一枪打中了 —— 尾段的真身是**缺 GPU 端点**，补上之后又抓到一个解码器缺档

### 8.103 run NS 把那条"8 秒忙等"的来路问到底：等的是 **GPU 的 idle 位**

站点挂在 AON 忙等助手入口 `0x0036FB90`（file 0xC0A8，换算器现算并回代自证），
`--site-after 2.5G`、预算 10，实测三笔：

```
第 2,520,156,981 条  r0=00010000  r4=00001000  lr=00390EB3  sp=3C762E10
第 2,920,156,980 条  r0=00020000  r4=00002000  lr=00390EB3  sp=3C762E10
第 3,720,156,980 条  r0=00040000  r4=00004000  lr=00390EB3  sp=3C762E10
        间隔 400M / 800M 条 = 4.0 / 8.0 设备秒，r2=40084000（AON 定时器）、r3=40220000
```

⇒ **等待量每次翻倍**（4.096 → 8.19 → 16.4 秒），是同一条任务（`sp=3C762E10`）在做指数退避重试；
`r3` 那位是 `0x40220000`。调用者的 `lr` 按 `0x363AE8` 反解会落到池字（上一轮已认定这条路不可信），
所以走**影子源法**：把停机那拍的 SRAM 段（`0x20190EAF` 起 8/12/16/24 字节）拿去全镜像搜同字节，
四种长度都命中 **file 0x2AEC7** —— 也就是这段倒灌代码的影子源在 flash 的 0x2AE00 一带。
Ghidra 读出来是一个自成一族的寄存器序列函数（`bl 0x2ADB8` 读、`bl 0x2ADC8` 写、`bl 0x2AB9C` 延时）：

```
0x2AEC0  movw r5,#0xb05            ; ★等的就是这几位
0x2AEC4  mov r0,r4 ; bl 0x2AB9C    ; 退避量 = r4（2,4,8,… × 0x8000 拍）
0x2AECC  movs r0,#4 ; bl 0x2ADB8   ; 读寄存器号 4
0x2AED0  bics r0,r5,r0 ; lsl r4,#1 ; bne 0x2AEC4     ; (掩码 & ~读值) 不为 0 就再退避一轮
```

**`0x40220000` 是什么，用权威源定，不按长相猜**：参考引擎的机型文件
`ref/bes-qemu-1502p/best1502p.c:467-478` 里，这一段就是 **GPU**：
`miband_gpu_init(...)`、`memory_region_init_io(..., "best1502p.gpu", 0x1000)`、
`add_subregion_overlap(sysmem, 0x40220000, ...)`、中断线 **70**（`irqs.gpu` 我方 profile 早就记着）。
而 `qemu/ip/gcnanoultrav/miband_gpu.c:10-23` 给的寄存器表是
`GPU_CLOCK_CONTROL=0x000`、`GPU_IDLE=0x004`、**`GPU_IDLE_STATE=0x0b05`**、
`GPU_CHIP_ID=0x020`、`GPU_REVISION=0x024`、`GPU_CID=0x030`、`GPU_ECO_ID=0x0e8`、
`GPU_INTR_STATUS=0x010`、`GPU_INTR_ENABLE=0x014`、`GPU_CMDBUF_ADDRESS=0x500`、
`GPU_CMDBUF_SIZE=0x504`、`GPU_SOFT_RESET=bit12`、`GPU_ISOLATE=bit19`，读分支 1986-2037：
**idle 只在 soft_reset|isolate 全清时回 0x0b05，否则回 0**；`intr_status` 读后清零。

★两条完全独立的来源在同一个数字上对上了：参考实现的 `GPU_IDLE_STATE = 0x0b05`
与 p65 固件 `movw r5,#0xb05` 那个等位掩码**逐字相同** —— 这不是巧合，是同一块 IP。

**所以 8.101/8.102 那条因果链补完并改定：**UI 任务不是在"睡觉"，是在**等 GPU 报 idle**；
我方没建 GPU，`0x40220004` 由兜底锁存回 0，于是它按 4/8/16 秒一路退避，
`miwear_init` 的刷新期限格 60 设备秒没人续，81 秒时看门狗 PANIC（8.99）→ board_reset
→ `Error: pmu wdt reboot failed!` → SRAM 收尾自环。**缺器件端点，不是软件闸。**

### 8.104 按四步规矩把 GPU 端点建起来（`--sabotage` 与全量回归都绿）

- profile：`best1502_r0.soc.json` 里把 `"gpu": {"base": "unmodeled"}` 那份占位换成实块
  （区间 `gpu` 0x40220000+0x1000 嵌在 mmio 里 + `devices.gpu` 的偏移/位号/就绪值/身份值，
  `_provenance` 写清两处出处）；`flint_profile.py` 的 MODELED 按"devices 里有且不是 unmodeled"纳入，
  校验器加五条判据（缺 off / 非对齐 / 落在窗外 / 两格撞同一偏移 / 就绪值与 isolate 位重叠 / 就绪值为 0），
  `--selftest` 加五条对应对照组（其中"没有同名窗口"这一档就是本轮实测的那个形状）。
- `src/l2/dev/gpu.{h,c}`：偏移、位号、就绪值、身份值全从 profile 读，代码里不出现硬件地址；
  idle 走参考实现那条判据；窗口内没建的偏移**交回锁存**（返回 false），不凭空造 0。
- `tests/unit/test_gpu.c` **39 项**：就绪判据那一条断言的是**固件的掩码 0x0B05**（不是 profile 值的回声），
  复位/隔离两位各自独立压住就绪、只读格写不动、身份格回读、cmdbuf 写长度算一次投活、
  窗口外与非对齐拒绝、子字节只给低 8 位；`--sabotage` 反写对照组退出码成立。
- `build.sh` 四处（DEV / 编译行 / [OK] 名单 / 运行行 + 对照组块），`test_boot.c` 的
  `wire_devices()` 里按"profile 里有 `gpu.off.idle` 才装配"接上并打名单，停机时 `fl_gpu_dump`。
- 全量 `bash flint-vm/build.sh` 绿。

**run NT（GPU 接上之后）**证实这道闸开了：设备热点表里出现
`40220020 读2 值=00000265`、`40220024 读2 值=00002002`、`40220030 读2 值=00000424`
⇒ **驱动过了 idle 等待之后接着去读 chip_id/revision/cid**，回读值与参考实现的默认身份值一致；
装配行 `接上 GPU：40220000（idle 格 +4 就绪值 00000B05，复位位 12/隔离位 19，身份 …）`。

### 8.105 下一道闸当场现形：解码器缺 **后索引减号档的 LDRD/STRD**

run NT 跑到 `2,155,314,422` 条停成 `停机=指令未实现`，`PC=0x0CA678A4`（= **file 0x9A78A4**，
正好在 8.86 那条屏幕服务链的 `FUN_009a7674` 同一个函数族里），编码 `015CE864`。
两个工具先对齐（纪律 17）：capstone（THUMB|MCLASS）与 Ghidra 在这一条上一致 ——
`strd r0,r1,[r4],#-0x170`，下一条 `strd r3,r1,[r4,#-0x8]`（0xE944 档，早已实现）。

位段**实测**，不按记忆写：用 keystone 正反编一组同形状样本 ——

```
strd r0,r1,[r4],#-0x170 = E864/015C      strd r0,r1,[r4],#0x170 = E8E4/015C
ldrd r0,r1,[r4],#-0x170 = E874/015C      ldrd r0,r1,[r4],#0x170 = E8F4/015C
偏移位宽：h1[7:0] = 0x5C，×4 = 0x170  ← 与两个反汇编工具报的立即数一致
```

判族收 `bit8=0、bit6=1、bit5=1` 三条形状位（bit7=U 加减、bit4=L 是 ldrd、h0[3:0]=Rn 全自由），
即把原来只收加号的 `(h0 & 0xFFF0)==0xE8E0/0xE8F0` 换成 **`(h0 & 0xFF60)==0xE860`**。
邻居逐条验过不被吞：`ldrex=E854`、`strex=E844`、`ldrexd=E8D4`（bit5=0），
`stmia.w=E8A4`（bit6=0），`ldmdb=E934`（bit8=1），`ldr/str` 的 F8xx 前缀（bit15..8 不符）。

ISA 向量补 14 条用例（后索引**减号**档 × strd/ldrd × 六组 Rt/Rt2 × r2/r9，
外加把 p65 那一条按原样收进去 —— 现在是 **CASE 812 `64E85C01`**，期望值来自 Unicorn oracle），
`bash flint-vm/build.sh` 全绿（`test_isa` 的对拍与"改掉一条期望值确实失败"的对照组都过）。

run NU = 同一套判据重跑（GPU 端点 + 解码器补档都在），看能走到哪：
判据仍是 `lcdc 帧>0` / `--panel` 出 PNG / 分派 `0x0C50F63C` / `screen_turn_on 0x0CA69200` /
`_assert 入口 0x0C4F74E8` 有没有命中，以及 `gpu dump` 的"投活"次数（命令缓冲执行仍未建模，
那是 GPU 之后的下一道闸）。

### 8.106 run NV：两道闸都让链过了，下一道闸还是解码器 —— 缺 **VCVT 定点档**

`--deep 2500000000`（约 3.3 分钟）读数：**执行 2,227,629,450 条后停机=指令未实现**，
`指令地址=0CA85F4A`（= **file 0x9C5F4A**，`addr_conv which` 现算现抄），下一条 `0CA85F4E`，
编码 `8ACCEEBA` ⇒ `vcvt.f32.s32 s16,s16,#8`。上一轮停在 2,155,314,422（LDRD 减号档），
这一轮多跑 72,315,028 条撞到下一档 —— **两处补的都是真闸，不是绕过去了**。

同一次的时间线（`--beat 250000000`）里几个还在动的信号：

```
250M  PC=0C50EA58 异常 进=5116 返=5116 pend=114  UART1=2850
2.0G  PC=0C512C6E 异常 进=21253 返=21253 pend=1220 UART1=3199
lcdc 帧=0 中断=0｜te 给=44 跳=4｜gpdma 段=0 搬运=0 字节（全程）
```

注意两点，别把这一轮当"进展"：
1. **2.2G 只等于 22 设备秒**，8.101 那条期限（60 秒）与 81 秒的 PANIC 都还没到 ——
   这一轮**证不了也否不了**"GPU 端点把 panic 消掉了"，那要跑够 9G 以上才算数（8.109 去做）。
2. `lcdc 帧=0` 仍是 0，`gpdma` 仍没搬过东西 ⇒ 显示链的产出还没有。

### 8.107 VCVT 定点档：上一轮那份"实测"作废，重做完才建（`test_vfp` 266 项全过）

**先收回自己的结论（纪律 18）**：上一轮 `_probe/probe_vcvt_fixed.py` 写的
"整数→浮点实测 = 值/2^N"那份读数**无效**，两个原因都在探针自己身上：

- 它只把输入 `movs r0, #3`，**从没 `vmov s16, r0` 装进 S 寄存器**，于是被测指令读的是复位值 0，
  整批"整数→浮点"的读数都是 0.0（0 除以任何 2^N 还是 0）—— 看着像测到了，其实一条没测；
- 结束地址写的是 `CODE + len(code) - 2`，而最后那条读回结果的 `vmov r1, s16` 是 4 字节，
  于是**停在它的中间**，`run()` 拿到的 r1 其实是上一条留下的。

这一版先把自证补上，再取数（`_probe/probe_vcvt_fixed2.py`）：
① 阳性对照 `movs→vmov s16,r0→vmov r1,s16` 往返，7 与 `0x41C80000` 都原值回来才继续；
② 每条用例按**字节**在整段程序里找回被测那条，出现次数必须正好 1（按 capstone 的文字比不行 ——
   它把 ≥10 的立即数印成 `#0x10`，比拼写会误判"没找到"）；
③ 末尾垫一条 `nop`、结束地址取 `CODE + len(code)`。

**位段（全扫，不测两端）**：把 N 从 1 扫到 31 逐档取编码，得到
`M = (h1[4:0] << 1) | h1[5]`、`N = 32 - M`（N=1→8AEF、8→8ACC、16→8AC8、31→8AE0）；
成员 = `{h0[2], h0[0]}`：`EEBA` f32←s32、`EEBB` f32←u32、`EEBE` s32←f32、`EEBF` u32←f32
（同一份 `h1=8ACC` 四个名字全对上）；`h1[8]` 是单/双精度档，`h1[7:6]=11`、`h1[4]=0` 是形状位
（翻掉任何一位两个工具都报"解不出"或跳去别的族）。

**★这一档只有 Vd，没有独立 Vm —— 是判别实验定的，不是"没测到"**。上一轮记的"Vm 位置未定"
来自 keystone 在这一档直接忽略第二个操作数（`vcvt.f32.s32 s0, s16, #8` 它照样编成 s0,s0），
所以"动 Vm 看哪几位跟"这条路对它无效。改成三选一对撞（`_probe/probe_vcvt_vm2.py`）：
预置 `S16=256、S8=3、S4=7、S24=11`，跑候选位型 `EEBA/8AC4`：

```
假设 A（低 6 位是 N、原地）  → 256/2^24 = 1.52588e-05
假设 B（(h1[3:0]<<1)|h1[5] 是 Vm）→ 读 S8 → 3/2^8 = 0.0117188
假设 C（Vm 在别处）           → 读 S4/S24 → 7/2^N 或 11/2^N
实测 1.52588e-05，且 S8/S4/S24 一字未动 ⇒ 假设 A
```

capstone 对同一位型报 `vcvt.f32.s32 s16, s16, #24`（原地）与 Unicorn 的读数一致，
keystone 的折叠是同一件事的另一面 —— 三个工具口径统一（纪律 17）。

**普查真身（`_probe/scan_vcvt_fixed.py`，判据与解码器逐字一致）**：

| 镜像 | 定点档真身 | 原地 | 非原地 | N 出界 | 双精度档 | 实测 N 取值 |
|---|---|---|---|---|---|---|
| p65 | 11 处 | 11 | **0** | 0 | 0 | 1, 8, 11, 12, 14 |
| 043 手环 10 Pro | 13 处 | 13 | 0 | 0 | 0 | 2, 8, 10, 12, 14, 17 |
| o66 手环 10 | 12 处 | 12 | 0 | 0 | 0 | 2, 8, 10, 12, 14, 17 |

第一版普查只按 `h0 ∈ EEBA/EEBB/EEBE/EEBF` 判，把 `EEBF/7A00`（其实是 `vmov.f32 s14,#-1.0`）
和 `EEBB/EEF9`（`mrc`）这类字节巧合全算了进来 —— p65 报 31 处里 20 处是噪声、M 谱也跟着失真
（"M=0 有 13 处"其实是 vmov 的 `h1=7A00` 混进来的）。**换成与解码器同一条掩码之后才干净**：
又一个"判据本身要对照"的例子（纪律 16）。

**两个不建的档都落未实现，且有出处**：`h1[8]=1`（双精度）三个镜像各 0 处；
`M=0`（N=32）也是 0 处 —— 而且这一档两个工具的读数与这里的公式谁也不符
（capstone 报 `#0x20`，Unicorn 把 1.0 算成 0.248047，两者都不是 `/2^N`），
按"未实现的位型必须停机"处理，不猜（陷阱 22）。

**建成之后（`flint-vm/src/l1/cpu/interp.c` 的 `ex_vfp` + `test_vfp.c` 新增 V17）**：
族判据 `(h0 & 0xFFBA) == 0xEEBA && (h1 & 0x0FD0) == 0x0AC0`；邻居逐条验过不被吞 ——
`EEB8/EEBC/EEBD` 靠 `h0[1]` 分开（vcvtr 那一族在三个镜像里有 509/1260/863 处，都不该进来）、
`vmov.f32 #imm` 靠 `h1[7]`/`h1[6]` 分开、VRINT 的 EE 档靠 `h0[3:0]` 分开。
语义两向都按实测：整数→浮点 = `值 / 2^N`（平局取偶，`16777217/2^8 → 47800000` 就是这条的证据），
浮点→整数 = `值 * 2^N` 先缩放、再按 `FPSCR[22:21]` 取整、最后夹范围
（为此把 `vfp_round` 的入参从 float 改成 double —— 缩放结果必须先取整再落回整数，
传 float 等于多舍一次）。
`test_vfp` 现在 **266 项**，其中 V17 把 p65 真身那 8 种位型逐条收进表
（期望值全部现测现抄，来源 `_probe/probe_vcvt_p65_population.py`；本轮手算错过两次：
`65536.0` 的位型写成 `47000000`、`256/2^8` 的读数写成 `1/256`，都是被实测逮回来的）。

**顺带记两个标签错**（不改代码，只免得以后拿旧笔记当读数）：
`0x42200000` 是 **40.0** 不是 10.0（上一版探针把它标成 10.0，于是"40*2=80"看着像读数错，
其实标签错、读数自洽）；`EEFA/8ACC` 是 **s17** 不是 s1 —— `h0[6]` 是编号的**最低位**，
s1 的位型是 `EEFA/0ACC`。两处都按实测改正。

**为什么 ISA 向量层没有这一族的用例（不是漏覆盖）**：`isa_vector.txt` 里 `vcvt`/`vmov`/`vldr`
一条都没有（grep 计数 0）—— 那套对拍格式只比 GPR 与 NZCV，纯 S 寄存器的结果在里面断不出来；
唯一进向量的 FP 用例是 VLDM/VSTM 那批，因为它们在基址写回与内存上留痕。
**标量浮点一族的判据归属 `test_vfp`**，记在这里免得以后再当缺口查一遍。

### 8.108 下一枪的判据（写死，跑完再评）

`bash flint-vm/build.sh` 全绿之后重跑长轮（记 **run NW**），只看这四条：

1. `2,227,629,450` 那一步是否不再停机；
2. **9G 以上**（≥90 设备秒）跑完之后，8.99 那条 `miwear_init.c:187` 的 PANIC 还在不在
   —— 这是"GPU 端点把整条尾段消掉了"唯一的判据，短轮不算数；
3. `gpu dump` 的投活次数 `n_cmd`（命令缓冲执行仍未建模，>0 就说明下一道闸是 GPU 干活）；
4. `lcdc 帧>0` / `--panel` 出 PNG。

### 8.109 run NW 读数：只跑到 2.228G 就撞上 **SMLABB**，GPU 那道闸的账还没结

`--deep 10000000000`（100 设备秒）实测 **执行 2,228,507,760 条后停机=指令未实现**，
`指令地址=0CA8DE1C`（= **file 0x9CDE1C**），编码 `310AFB11` ⇒ `smlabb r1,r1,sl,r3`。
比 run NV 只多走 878,310 条 —— VCUT 定点档一让开，下一档就顶上来。

同一轮的 GPU 读数（`fl_gpu_dump`）：

```
gpu 读10 写8 只读格被写0 没建偏移0 宽度不认识0
clock=00000100 idle读回=00000B05 中断=00000000/7FFFFFFF  投活0 长度0
```

读法：驱动把中断使能整片开了（`7FFFFFFF`），idle 已经读得到就绪值，**但命令缓冲一次都没提交**
（`+0x500/+0x504` 零笔）⇒ 8.108 那四条判据里，"PANIC 是否消失"这一条本轮**既没证实也没否证**
（22 设备秒离 81 秒那条期限还远），"投活次数"是 0 ⇒ GPU 的渲染/完成中断这道闸还没轮到。

**来路逐条核过，不是"跳进数据"**（这一条必须查，因为 `smlabb` 看着像数据的概率不低）：

1. file 0x9CDB54 有完整序言 `push.w {r4,r5,r6,r7,r8,sb,sl,fp,lr}`，往后 `cmp/beq/bl` 成一条连贯的
   表走查（`movs r1,#0xc`、`movs r3,#0x2c`、`ldrb.w sl,[r4,#0x28]`、`smlabb r1,r1,sl,r3`）；
2. 停机点前一条是 `0x9CDB94 cmp.w fp,#0x2b` + `0x9CDB98 bgt.w 0x9cde14`，
   我方偏移式算出的目标与 capstone 报的是同一个（`#0x27c` 是它按"地址 0 + PC+4"给的**目标**，
   不是偏移 —— 一开始我把它当成偏移比，得出"差 4"的错误结论，量完才发现两式一致）；
3. 再往前是 `FUN_0086ba7c`（file 0x86BA7C）结尾的 `pop.w {r4,r5,r6,r7,r8,pc}` 返回到
   `0x0CA8DB99`，而 Ghidra 列出的**这个函数的全部引用点都在 0x9CADxx / 0x9CBACx / 0x9CCxxx 那一段**
   ⇒ 返回地址落在 0x9CDBxx 正是应有之义。

**★收回上一轮自己的定性（纪律 18）**：上一轮 `--through-unimpl` 枚举出 6 种未实现位型，
其中 4 条是 `smlabb` 一族，我当时判成"状态漂移产物 —— M-profile 没有媒体指令，不算缺口"。
**那个定性是错的**，两处错：
① 同族的 `smulbb`（真镜像 0x0C1D8726 = FB13/F101）早就确认过并且建成了 ——
   说明这一片属于 **Armv8-M Mainline 的 DSP 扩展**（`+dsp`，Cortex-M33 可选实现，
   BES 这颗就是开着编的），不是 A/R 独占的媒体指令；
② 我把陷阱 35 那句"Unicorn 定不了架构是否认领"反过来用了 ——
   它只禁止"拿 oracle 能跑通当存在证据"，**不构成"oracle 读数怪就等于这条不该存在"**。
   把真缺口判成产物，代价是后面每一轮都在同一条上重新撞。

存在性这次换成三件独立的事撑：keystone 原样编出（`smlabb r1,r1,r10,r3` → `11FB0A31`，
与固件那两个字节逐字节相同）、capstone 与 Ghidra 两个反汇编器同名、而且它在真函数里可达。

### 8.110 SMLAxy 建成（`test_mac` 29 项，语义等级 = 架构定义，不采信 oracle）

**位段实测**（逐位翻读数看 capstone 的读数怎么变，`_probe/probe_smlabb_bits.py`）：

```
h0 = 0xFB10 | Ra            bit4=1 是这一族与 mla 的分界；bit5→smlawb、bit6→smmla（都没建）
h1[11:8] = Rd  h1[3:0] = Rn  h1[15:12] = Rm  h1[5:4] = xy  h1[7:6] = 0
xy：bit5 选 Rn 的高半字、bit4 选 Rm 的高半字（与已建的 SMULBB 同一套读法）
```

与 SMULBB 的分工要说清：**SMUL 是 3 操作数，`h1[15:12]=1111` 是形状位、`h0[3:0]=Rn`；
SMLA 是 4 操作数，同一个 `h1[15:12]` 变成 Rm、`h0[3:0]` 变成累加源 Ra**。
两族判据正好互补（一个要 `==0xF000`、一个要 `!=0xF000`），`Rm=15` 那种本来就没法区分，
架构也不允许，所以不存在互相抢的情况。

**语义**：`Rd = Ra + (int16)Rn_x * (int16)Rm_y`，不动 APSR，不夹范围（32 位截断）。
这条**等级 = 架构定义 + 位段实测**，因为 oracle 在这一族不可信 —— 实测 Unicorn 四档给

```
smlabb→000B0008  smlabt→000B000C  smlatb→000B0005  smlatt→000B0005
（Rd=0x11111111、Ra=1、Rn=0x00070003、Rm=0x000B0005）
```

结果基本等于"第四操作数加点东西"，换一组输入（Rn 全负）四档读数还完全相同 ——
那是它按 A/R 档的另一套字段位置在算。**只有 APSR 不动这一条与架构相符**，保留。

**顺带把第一版语义探针的错记下来**（同一类纪律 16 的错，我隔了一轮又犯一次）：
那版把 `Rd` 与 `Ra` 设成同一个寄存器（`smlabb r1, r1, ...`，固件那条恰好就是这样），
于是"累加源到底取谁"分不开；而且我拿 `movw/movt` 的拼法**假定**输入值、没把寄存器读回来核对，
所以那批"预测 vs 实测"的对比是无效的。现在 `test_mac` 里用 `Rd=0x11111111、Ra=1` 的组合
把这一条钉成判据（结果必须正好是 `Ra+乘积`，否则就是拿 Rd 当累加源）。

**门（新建 `flint-vm/tests/unit/test_mac.c`，29 项）**：
M1 固件原样那条（`FB11/310A`，12+3*44=144，邻居 r2 不许动）；M2 四档正半字（16/34/36/78）；
M3 带符号那一维（-15/-33/-35/-77）；M4 先立 NZCV 再跑、比回来（并断言对照立得起来）；
M5 相邻族互斥 —— `smulbb FB13/F101` 仍按 smul 算（15，不累加）、`mla FB04/6305` 仍按 mla 算（5）、
`Rd=15` 必须停成未实现。`--sabotage` 反写两条成立。
`build.sh` 四处都接上（编译行 / `[OK]` 名单 / 运行行 / 对照组名单）。

**ISA 向量为什么不收这一族**：向量的期望值来自 Unicorn，而它在这一族按 A/R 档解码 ——
收进去等于把错的读数当判据（陷阱 24 的同一件事，`claim=2` 那一档就是给这种用例留的）。
所以这一族的判据归 `test_mac`，期望值按定义手算并写明算式。

### 8.111 下一枪（NX）的判据

跑够 **9G 以上**（≥90 设备秒）才是 GPU 那道闸的账；四条判据不变（8.108）。
若又撞见"看着像媒体指令"的未实现位型，先做 8.109 那三步来路核对（序言、前一条、返回地址归属），
再按"汇编器原样编得出 + 两个反汇编器同名"定存在性，**不要**再写"漂移产物"。

### 8.112 run NX：解码器闸全开了，停点换成**设备拒答**（GPU +0x0C），而且 GPU 第一次投活

`--deep 10000000000` 实测 **执行 2,385,150,584 条后停机=总线错误**（`未实现 0`，两处解码器缺档都不再出现）：
`指令地址=00390DA6`（SRAM 执行别名），`最后一次坏访问 addr=4022000C 类型=设备拒绝`，
停机帧里 `r3=40220000`、`r0=0000000C`，GPU 自己的计数 **"没建偏移 1"** ⇒ 驱动问的是 **GPU+0x0C**。

同一轮 **GPU 第一次投活**：

```
gpu 读11 写10 只读格被写0 没建偏移1｜clock=00000100 idle读回=00000B05
中断=00000000/7FFFFFFF｜投活1 最近=384DF980 长度17
```

`+0x500` 写进去的是 `0x384DF980`（PSRAM 的 0x38 别名）、`+0x504` 写 17 条 —— 命令缓冲真投了。
三个判据站（`0x0C50F63C` 分派 / `0x0CA69200` screen_turn_on / `0x0C4F74E8` _assert）本轮**零命中**；
注意这是阴性结论且本轮**没做阳性对照**，按纪律只记"没看到"，不当"从未执行"。

### 8.113 GPU 窗口内没点名的偏移：权威口径是**设备自己答 0**，不是拒答

依据在参考实现的机器层包装（`best1502p.c:219-232`）：`value` 初始化 0，调用
`miband_gpu_read/write` 之后**不看返回值**（那两边对没点名的偏移都是 `default: return false`）。
QEMU 里没有"这笔访问被拒"这种形态，所以固件在真机上读 +0x0C 拿到的是 0。

**★收回上一轮那句判据注释**（"没建模的格子要交回锁存，不能凭空造 0"）：在 flint 的区间模型里，
区间被设备占住时"交回锁存"没有落点，直接变成总线错误 —— run NX 就是这么停的。
改法（`src/l2/dev/gpu.{h,c}`）：窗口内没点名的对齐偏移 → 读回 0、写吞掉，**同时把这几种偏移
逐个记进表里并随 dump 打印**（不安静放过）；窗口长度改成从区间表读（不再写死 `0x1000`）。
`test_gpu` 39 → **42 项**：G11b-G11e 断言"回 0 / 吞写 / 不锁存 / 表里记着 +0x0C 读2写1"，
G11 那条旧判据作废；`--sabotage` 加一条反写。

### 8.114 run NY：跑到 55 设备秒，GPU 投活 26 笔；停在新的一条 16 位指令

`--deep 10000000000` 实测 **执行 5,506,481,008 条**（= **55.06 设备秒**）后
`停机=指令未实现`，`指令地址=0C569750`（= **file 0x4A9750**），编码 `0000BAC9`。

同一轮的 GPU 读数（这条比停点更重要）：

```
gpu 读6597 写164｜投活26 最近=3851FA00 长度16｜中断=00000000/7FFFFFFF
没点名的偏移共 13 笔、列出 1 种：+00C 读13写0
```

三件事：① **命令缓冲被反复提交（26 笔）**且驱动一直在往前走 ⇒ **它不等完成中断**
（状态位始终 0 也能继续）—— 此前"下一道闸是完成中断"的预测**作废**；
② `+0x0C` 是只读的（13 笔全读），答 0 之后驱动没有卡住 ⇒ 那格大概是能力/配置查询；
③ 55 设备秒仍然 `lcdc 帧=0`、`gpdma 段=0`、`UART1` 停在 3216 字节 —— 显示链的产出还是没有，
但 UI 任务已经活过了 8.101 那条 60 秒期限的一半以上，**PANIC 判据要等这一轮（run NZ）跑完**。

### 8.115 补 REVSH（16 位档）+ 一条"工具速度"教训

0xBAC9 = `revsh r1,r1`（capstone 同一份字节同解；`0xBA80` 它解不出 ⇒ op=10 仍未分配，继续停机）。
`interp.c` 的 `revfam` 分支原只收 op8/op9（rev/rev16），补 op11：
`revsh` = 低半字字节倒序后**符号扩展**到 32 位。
生成器的 16 位档循环加上 `revsh`（含 `rd==rm` 那一组，正好是固件那条 `BAC9`），
重生成后 `isa_vector.txt` **2588 条判定、50021 项断言全过**。
（生成器结尾"未认领族"里列了 `rev/revsh` 是它自己的记账噪声：那一列只影响统计，
claim=0 的用例照样重放判定 —— 别当成漏覆盖。）

**速度**：这几轮慢的一大半不在分析，在**每次查询都新起一个 headless JVM**。
`ghidra_fw.py fns <目标> 0xAA,0xBB,0xCC` 一次会话查多个地址（实测三地址 25 秒，
单条 `fn` 40-60 秒/条）。已补进 `ghidra-firmware-analysis` 技能。

**REA（morluto/rea）评估**：装上 `rea@6.5.0`（`npm i -g rea-agents`，Node 22+），
CLI 支持 `analyze --target-format raw-binary --architecture arm --entry-point`，
但在 **Windows** 上被它自己的适配器拒绝：

```
ghidra:  target_format_unsupported — Windows Ghidra P0 accepts native PE and explicitly
         selected raw 6502 targets only
hopper:  unsupported_host（需 macOS）
ida:     not_configured（需 REA_IDA_MCP_CONFIG）
```

另：rizin/objdump 在 REA 里只做调试命令面，不做分析 provider。
⇒ **裸 ARM 固件这条路目前在本机不通**；要用得换 Linux 主机（Ghidra 后端支持 raw+arm）
或装 IDA 并配好它的 MCP。结论落盘，别重复试。

### 8.116 帧请求那一格到底是谁写的：`FUN_008a89cc` 已读全（顺带收回一处地址口径错误）

**先记一次自己的错**：本轮一开始把文档里的运行地址 `0x0C9689CC` 当成文件偏移去查了
`0x9689CC` —— 那是另一段代码（一个走 `.` 分隔串的路径工具），得出了"帧请求函数其实是
字符串工具"的错误印象。用 `addr_conv.py which` 现算才回到正轨：**file = 0x8A89CC**。
（`0x8A89CC + 0x0C0C0000 = 0x0C9689CC`，回代自证通过。）这也解释了为什么那次查询
的"引用者"名单与 §8.24 对不上 —— 不是工具错，是口径错。

**`FUN_008a89cc` 的完整语义**（Ghidra 反编译逐字）：

```
临界区（取 BASEPRI 存起来、写 0x80）
if (*(u8*)(DAT_008a8a0c + 0x150) < 2) {          // 显示设备对象、+0x150 是"在飞的帧数"之类
    FUN_007fef50(0xb, 7, param_3, *(u8*)(obj+0x150), param_4);   // 走 0x7FE000 那条 libc 腿（日志/事件）
    FUN_008a84b0(3, 1);                          // 引擎登记（它内部调 FUN_008adcf8，见 §8.24）
    *(int*)(obj + 0x154) += 1;                   // 请求计数
    *(u32*)(obj + 0x148) = 1;                    // ★帧请求位
}
恢复 BASEPRI
```

它只有两个引用点，两个都对得上 §8.24 的动态记录：
`from 0x00890556 (FUN_00890470 = logo 渲染函数)`、`from 0x0044F72A (FUN_0044f63c = 消息分派)`。
`DAT_008a8a0c` 里的值就是 **0x3C3CA330**（§8.24 说的显示设备对象），而这个全局
**全镜像只有这一处引用** —— 说明 `+0x148` 的消费者不是从这个全局拿对象的，
它手里有另一条指向同一对象的路径。要定位消费者，得先找到"谁持有 0x3C3CA330"，
不能靠 xref 这个全局。

**LCDC 基址常量（0x40200000，小端字面量）的全镜像普查**：**48 处**，其中 **38 处**在
file 0x8B1D00~0x8B2900（就是实测"31 个寄存器格被碰过、写 PC 全在 file 0x8B1Dxx"那段 LCDC
初始化，PC 0x0C971Dxx-0x0C9729xx），另外 **10 处**在
`0x1AAF52 / 0x250D04 / 0x254911 / 0x254EB5 / 0x449041 / 0x894854 / 0x8A897C / 0x8B2944 /
0x9E4C70 / 0x9E4D28`。逐个查过含它的函数：`0x894854`、`0x8B2944`、`0x9E4C70`、`0x9E4D28`
**都是数据（Ghidra 建不出函数）**；`0x8A897C` 那个现场建的函数反编译成一堆 `unaff_r4/r5/r7`
残句（把池字当代码了），**不可信**。
⇒ **目前没有任何第二处代码被证实会在运行期写 LCDC** —— 与 `lcdc 帧=0` 一致。
**但这条阴性有前置条件**：若驱动把 LCDC 基址存在 RAM 结构里、之后靠指针访问，
常量就一次都不会出现 —— 所以它只能说明"没有第二处**字面量**",不能说明"没人写"。

下一步（等 run NZ 的读数一起判）：
1. 若帧请求 helper 在 GPU 投活之后仍有命中（§8.24 记录过 2 次早段命中），
   说明请求在发、消费者缺位 —— 去找"持有 0x3C3CA330 的那条路径"；
2. 若它不再命中，说明请求路径本身在上游断了（谁该调 FUN_00890470 / 0xFD 分支）。





**补一句"消费者找到了"（同一轮的静态收尾）**：显示设备对象 `0x3C3CA330` 的指针值在全镜像
出现 **30 处，全部落在 file 0x8A7778~0x8A8ADC 这一个模块的字面量池里**（说明这个模块
自己带了一整套指向该对象的常量，`DAT_008a8a0c` 只是其中一处）。其中一条池字后面就是
**`0x8A8A96` 的 `LDR.W [r3,#0x148]`** —— 它落在**搬帧函数 `FUN_008a8a64`（0x8A8A64 起）内部**，
而搬帧的唯一调用者是消息分派 `FUN_0044f63c`（0xFD 档）。⇒ **"谁消费帧请求位"这条已经闭合**：
消费者就是搬帧函数，门在 0xFD 那一档；**缺的是 0xFD 的生产者**，不是消费者。
（注意：读 `+0x148` ≠ 会去写 LCDC `+0x224`；搬帧函数体里有没有触碰 LCDC，下一轮读它时确认。）

顺带一条口径澄清：`p65.assert_entry` 记的 `0x0C4F74E9` 是**运行地址**（= file 0x4374E8|1），
不是 file 0x4F74E8 —— 后者对应的运行地址是 0x0C5B74E8，是另一处代码。
本轮我一度按"file 0x4F74E8"去推理，差点把整条断言哨兵的读数判成假的；地址表的口径是运行地址，
现算一律走 `addr_conv`。

### 8.117 两个 beat 热 PC 各是谁：一个是"续期限"的，一个证明分派是**间接调用**

run NZ 的前几条 beat 里反复出现 `PC=0C50EA58 / 0C50EA8A`（同一条 `FUN_0044ea48`，file 0x44EA48）。
反编译看它做的是：取一个时钟（`thunk_FUN_0036fb78`）、在临界区里更新两个全局计数与一个时间戳。
这正是 §8.101 那条"UI 刷新期限"的续期动作 —— 它在跑，说明**60 秒期限在被续**，
与"GPU 端点补上之后 PANIC 不再出现"的预期一致（最终以整轮跑完的 `_assert` 站点计数为准）。

另一件事更关键：**消息分派 `FUN_0044f63c` 的入口在全镜像"0 处引用"**（Ghidra：该地址的数据未定义）。
它当然仍会被调用 —— 只是**间接调用**（任务入口表/函数指针那一类）。所以
"分派入口零命中"这句话的真实含义要升级：**不是"0xFD 没人发"，而是显示服务这条任务根本没被调度到**。
下一步要么找到安装这个函数指针的地方（任务表），要么从调度侧证明该任务为什么从不运行。

（顺带：`FUN_0044ea48` 这类"每拍都在跑"的函数会让 beat 的 PC 看起来高度重复，
别把它误读成"卡在同一处" —— 判卡死看的是异常计数与目标状态，不是 PC 重复。）

### 8.118 `0x0C50F63C` 其实是**表里的一项**：标签要降级，判据含义也要跟着改

扫"谁存了这个入口的指针"（按 |1 的 Thumb 指针值找）：`0x0C50F63D` 全镜像**只有 1 处**，
在 file **0x3E9544**，而它周围是一张表：

```
file 0x3E953C  0C50F5FD   (FUN_0044f5fc|1)
file 0x3E9540  0C50F615   (FUN_0044f614|1)
file 0x3E9544  0C50F63D   ★ 就是被我们叫"消息分派"的那个 0x0C50F63C
file 0x3E954C  00697073   ← ASCII "spi"
```

表本身**没有被任何指令静态引用**（`0x0C3E953C` 与它的 cache 别名各 0 命中）⇒ 它是
"基址 + 索引"方式使用的。所以：

1. `0x0C50F63C` 是**某张表的一项**，不是任务入口；"消息分派"这个标签来自上一轮对
   0xFB/0xFD 分支的静态判断，**现在降级为待核**（也可能是某驱动的 ops/ioctl）。
2. 站点在它上面零命中，只能说明"**这一项没被调用过**"，不能再说"0xFD 没人发"，
   更不能再往上推"显示服务任务没被调度"—— 这三句是三级不同的结论，之前混成了一句。
3. 要把它钉回去，按顺序做：① 认出这张表是谁的（看 `FUN_0044f5fc` / `FUN_0044f614`
   这两个邻居的语义，以及 "spi" 串属于表内还是下一个结构）；② 找到"基址 + 索引"的算法；
   ③ 才能判断哪条路径会调到它。

这一条是这轮第二次"标签先行"的教训：先起名（消息分派）再找证据，一旦地址换了口径，
名字就会把判据带偏。名字只写"地址 + 表项"，语义等证据。

**这张表长什么样（同一轮补齐）**：`0x3E953C` 那一组是**三条一组 + 一个设备名**：

```
0x3E953C 0C50F5FD   0x3E9540 0C50F615   0x3E9544 0C50F63D   0x3E954C "spi"
```

三条各自的语义读出来了：`FUN_0044f5fc(obj,arg)` = 「若 obj+0xc==0 则 obj+8=arg、obj+0xc=1」（attach），
`FUN_0044f614(obj)` = 「若 obj+0xc!=0 则清零」（detach），第三项就是被我们盯着的 `0x0C50F63C` —— 这是
**一套 attach/detach/transfer 的 ops**，设备名 "spi"。再往后 0x3E9600 起还有一张 **8 条指针**的表
（`0C50FC9D 0C50FCED 0C50FB89 0C50FC35 0C50FCB1 0C50FB99 0C50FD31 0C50FBA9`），
其中 `0x0C50FCED` **正是 043 的 fw_api 里 `home.BACKGROUND_HOME_ORIGINALS` 用到的地址之一** ⇒
这一段是 miwear 的 home/背景相关 ops 表族。

所以 `FUN_0044f63c` 所在的模块就是 miwear 的显示/背景一侧；它同时被记在"0xFB/0xFD 分支"里 ——
两个来源都指向"显示帧路径"，但**函数名/角色仍按表项写**，不叫"消息分派"。

**站点预算是"全局共用一个"的（下一轮别再踩）**：`test_boot.c:398` 的判据是
`if (g_site_hits++ >= g_site_max) { g_site_n = 0; return; }` —— 一次运行里**所有站点共用一个计数**，
且**任何一个站点把预算用尽，全部站点一起下线**。所以"挂四个站点、每个给 4"是危险写法：
帧请求那种早段就会命中的站点一旦用满 4 次，`_assert` 的观察窗当场失效，
读数看起来像"没有 PANIC"，其实是探针自己关掉了。多站点运行一律给足预算（如 200），
读日志时按站点地址逐条筛，不要指望"每站 4 次"。

**★再收一次自己的话（同一条，两小时内第二次）**：§8.118 开头写的"「消息分派」标签降级为待核"
**过头了**。地址表里 `p65.ui_msg_dispatcher_shape` 那一行早就把形状读全了：r0=会话对象、r1=消息结构，
`cmd>=0xF0` 走 `tbb` 五档直调（0xF0→008A8A10、**0xFB→008A89CC 帧请求**、0xFC→008A8A30、
**0xFD→008A8A64 等帧**、0xFF→008A8A20）—— 它就是**显示消息分派**，标签不用降。
真正该改的是同一行里那句 **"入口只能运行期填"**：按**运行地址**形式搜 `0x0C50F63D` 有 1 处静态值，
在 file 0x3E9544，且同一张表还带 attach/detach 与名字 "spi" ⇒ 入口有静态初始化，只是运行期零命中。
（原行搜的是 file 偏移形式 0x0C44F63D，所以搜不到 —— 又是一次"口径不同结论就反"的实例。）
所以 §8.118 的三级含义要重排：**分派是分派（成立）**；**表项静态存在（成立）**；
**它从未被调用（3.2G 实测零命中，成立）**；只有"角色不明"这条是错的。

### 8.119 五道出帧闸的现状（把散在全文的读数收成一张表）

按 `flint-firmware-bringup` 那张"出帧闸"清单对一遍，省得每次重读全文：

| 闸 | 现状 | 依据（都在这份文档里） |
|---|---|---|
| 面板认不出 | **已过** | DSI 应答拼出 rm690c0-VSN，`read g_lcd_module_id=0x00200135` |
| 命令没握手 | **已过** | DSI `+0x18` 写、应答 3 包；`0x35` tear-on 被状态机认出 |
| TE 没进 | **已过** | te_gpio=59 定对之后 GPIO 中断一路涨（run NZ 3.2G 时 116 次） |
| 帧完成位没置 | **未过** | `lcdc 帧=0`、`中断=0` 全程不变；`+0x224` 从没被写过（模型自己计数） |
| 请求挂着没人搬 | **半过** | 帧请求 helper 实测两次（252.4M、2.113G，同一 `bl`）；搬帧 `FUN_008a8a64` 一条都没跑。**第 30 轮补**：系统在 5.05G（50.5 设备秒）被 BT HCI 断言打断、随后进收尾自环，所以**5G 之后的"没跑"也可能是系统已经死了**，不能全记在显示链头上 |

所以问题收敛在最后两道，且第二道是"消费侧没被叫到"（0xFD 那一档没来过），
不是"请求没发"。下一步要么让 0xFD 出现，要么证明它本该由谁发。

### 8.120 帧启动的权威判据、以及两条"看起来像服务没跑"的证据其实不成立

**帧启动的唯一判据（权威源）**：参考实现 `miband_lcdc.c:1131`
`if (offset == LCDC_FRAME_START && (value & 1))` 才排一帧任务，`scanout_frames++` 在任务完成处（:528）。
⇒ 写 `+0x224` 且 **bit0=1** 才是"踢一帧"，我们模型与判据都对。

**p65 侧谁可能写 `+0x224`**：全镜像按 `STR.W Rn,#0x224` 形状扫到 21 处，其中**只有
file 0x8B2852 落在 LCDC 初始化函数内**（r3 基址），其余 20 处基址各异、没有一处被证明是 LCDC。
⇒ 客户端侧没有任何"写 +0x224 提帧"的代码。与 `lcdc 帧=0` 一致，且说明**不是我们的判据错**。

**两条被我当成"显示服务没跑"的证据，查下来都不成立**：

1. `screen_turn_on`（0x9A9200）零命中 —— 它**不是一个任务循环**：xref 列出 14+ 个调用点
   （file 0x9A24D8 / 0x7D1BF8 / 0x765CC8 / 0x76568C / 0x76B208 / 0x9A1FFC / 0x7233E4 /
   0x7B74C4 / 0x74A470 / 0x777160 / 0x77608C×2 / 0x779558 / 0x7A1358 …），散布在各个应用/控件里，
   是"用户交互要亮屏"时调的 API。模拟里没有触摸输入，它零命中是**应有之义**，不是证据。
2. 消息分派零命中仍然成立（3.2G 一次没到），但它的含义只是"0xFD/0xFB 没来过"，
   不能再外推成"显示服务整条任务死了"。

**当前最可能的形状**：UI 在 GPU 上画（投活 26 笔），但**没有人去踢 LCDC 的 +0x224**；
踢它的那一方要么是一个尚未被调度到的客户端，要么根本不走 LCDC（但参考实现里 best1502p 的
`scanout_bridge=false`，所以"不走 LCDC"这条路没有权威支撑）。下一枪按这个二分法取证。

### 8.121 要重新审一次"首帧判据"本身：p65 的面板可能是命令模式（待核）

三条已核的事实放在一起，`lcdc 帧>0` 这条判据对 p65 可能**根本不是正确的观测点**：

1. 参考实现的 LCDC 只对这几个写有反应：`0x0f0/0x0f4`（层配置）、`0x280` bit0（面板命令触发）、
   `0x224` bit0（排一帧，`miband_lcdc.c:1131`）。别的格都是普通锁存。
2. p65 全镜像的 `+0x224` 写只有 21 处，其中只有 1 处在 LCDC 初始化函数里，**客户端侧一处都没有**（§8.120）。
3. GPU 那一栈（HAL → `FUN_0002afc4` → 读/写访问器）**一次都不碰 LCDC**：
   `FUN_0002adc8` 的全部调用者只有 `FUN_0002ae3a`（上电）与 `FUN_0002afc4`（操作分派），
   而 `FUN_0002afc4` 的 12+ 个调用者全在 file 0x231xx~0x23Cxx（VGLite HAL 层）。

⇒ 如果 p65 的显示是"**命令模式面板 + 自己的 GRAM**"（面板 ID 是 rm690c0 那一族，
这条属于**外部常识、待核**，本仓库还没从固件侧证），那么像素应当以 DSI 长包
（DCS `0x2C/0x3C` 一类）送进面板，LCDC 的 `+0x224` 永远不会被写，
`lcdc 帧` 也就永远 0 —— 我们一直在等一个**不会来的信号**。

**要做的下一步（不是再等）**：
1. 给 DSI 模型加"像素载荷计数"：长包（`0x39` 头）里带 `0x2C`/`0x3C` 的、长度 > 16 字节的，
   单独计数并 dump（现在的 `te 给/跳` 只数了 TE）；
2. 用这个计数当 p65 的新首帧判据（同时保留 `lcdc 帧` 作对照）；
3. 若初始化之后 DSI 一次像素长包都没有，那问题仍是"没人把渲染结果送给面板"，
   与"没人踢 +0x224"是同一件事的两个观测面。

**这是一次判据修订，不是又一条推论**：上面的 1-3 都是已核事实，"命令模式"那一句标了待核，
等 DSI 计数出来再定。

**§8.121 的判据已落地**（`src/l2/dev/dsi.{h,c}` + `test_dsi`）：`0x39` 头里记长度
（`(len+1)` 的 8 位字段），紧跟的 FIFO 命令若属 `2C/3C` 或长度 > 16 就记一笔
`n_pix_pkts / n_pix_bytes`（2C、3C 各记条数），dump 多打一行
`dsi 像素载荷：长包N 共M字节（2C=… 3C=…）`。单测 110 项：200 字节长包算一笔、
字节数对、短命令 `0x15` 不算（负对照）。
（写测试时我先用了 257 字节，被 8 位长度字段截成 1 —— 是**测试输入越界**，模型的截断是对的。）
下一次重编后的跑批就按这一行判：非零 ⇒ 命令模式面板确实在收帧；为零 ⇒ 渲染结果从未送出。

### 8.122 帧请求不是"只发一次"：短枪实测两次，且两次同一个调用点

`--site 0x0C9689CC 200`（预算给足，避免全局预算把站点一起关掉）跑 3G，命中 **两次**：

```
第 252,428,474   条  lr=0C95055B  pc=0C9689CC
第 2,113,282,794 条  lr=0C95055B  pc=0C9689CC
```

同一个 `lr` ⇒ **同一个调用点**（file 0x89055B，在 `FUN_00890470` 内，与 §8.24 记的 0x890556 那条 `bl` 一致）。
所以"帧请求只在 logo 路径发一次"这句**要改**：它会发，而且是同一个调用者隔很久再发一次
（两次之间没有别的调用者）。

两次的场景寄存器不同（r0=3C2C3B90 → 3C2B5080、r2=0000AC20 → 0000AF80 等），说明是两块不同的
渲染目标各请求了一次，不是同一笔重放。

仍未变的是：**搬帧 `0x0C968A64` 到 2.5G 为止一次没跑**。所以"请求发了、消费侧没被叫到"这条成立，
只是请求侧比之前以为的活跃（两次）。

### 8.123 run NZ 判据：**miwear 的 PANIC 没了**，但 50.5 秒时换 **BT HCI 发送超时**炸掉

run NZ（GPU 诊断完成位之后的第一次长跑）读数：

```
_assert 0x0C4F74E8 命中 1 次 @ 5,052,112,067 条（= 50.5 设备秒）
   lr=0C512753  → file 0x452752（bl _assert 在 0x45274E）
5.5G 起 PC=0036EB36（已知的收尾自环）LR=0036E5A7，异常计数冻在 62,692 不再涨
```

**断言参数解出来了**（方法：VA − 0x3C000000 + 0x2FDC4 → file，再按 C 串读）：

| 参数 | VA | 内容 |
|---|---|---|
| 文件串 | 0x3C18254C | **`chip/bes_serial_hci2.c`** |
| 行号 | — | **0x12B = 299** |
| 日志 tag | 0x3C182604 | **`bthci_send`** |

**条件从指令直接读出来**（file 0x452728 起）：

```
bl 0x406de0            ; nxsem_timedwait 一类（同一函数在搬帧里也用来带超时等待）
adds r0, #0x6e         ; r0 + 110
bne  0x45262e          ; ≠0 走正常路径
... log(3, "bthci_send"...)
movw r1, #0x12b        ; 299
bl   0x4374e8          ; _assert
```

`adds #0x6e` + `bne` ⇒ **只有返回 -110（ETIMEDOUT）才 assert**。所以这是
**`bthci_send` 里的定时等待超时**：等的是 `r4+0x34` 那个对象（信号量/队列），
超时之后直接断言、PANIC、board_reset，最后落进收尾自环。

**两条判决**：
1. **§8.101 那条 `miwear_init.c:187` 的 PANIC 不再出现** —— GPU idle 端点确实把它消掉了
   （从 3.2G 到 5.05G 之前 `_assert` 零命中；50.5 秒那次是另一个文件、另一个行号）。
2. 现在的死因换成 **BT HCI 发送路径等不到对端**。这与"核 2 对端"是同一类问题：
   `bthci_send` 要等发送完成/额度，而 BT 那侧的消费者（另一颗核或外部芯片）在模拟里不存在。

下一步：找出 `r4+0x34` 是谁的信号量、谁本该 post 它（BT 核的发送完成中断 / 邮箱），
再按"对端应答"建端点。**在此之前，任何 ≥50 设备秒的跑批都会在这里死**，
所以首帧判据要在 50 秒之前看，或者先把这条对端补上。

### 8.124 `bthci_send` 的真入口与它等的那一格（file 0x4525E8）

**入口是这么找到的**（记下来，下次别再用"暴力对齐"）：按 2 字节起点穷举"能一致解到 0x45274E"
得到 393 个候选，等于没筛；换成**序言扫描**（`push {..,lr}` 的 `0xB1xx` 形状与 `push.w`
的 `E92D` 带 lr 位），在 0x451800~0x452728 里拿到 75 个，再挑"从这里解能连贯走到 assert 那条"的，
落点 **file 0x4525E8**（`push.w {r4,r5,r6,r7,r8,sb,sl,fp,lr}` + `sub sp,#0x1c`），
assert 在它的 +0x166（0x45274E）。

**反编译读出来的守卫**（逐字）：

```
if (param_1[0xc] != 0                       // ctx+0x30 标志
    && *(u8*)(param_1 + 0x31) == 0          // ctx+0x31 标志
    && nxsem_timedwait(param_1 + 0xd, 100000) == -110)   // ctx+0x34 信号量，超时值 0x186A0=100000
{
    log(3, "bthci_send", ...);
    _assert("chip/bes_serial_hci2.c", 299, 0);
}
...
thunk_FUN_0036c670(0x3C182520 /* "%02x " */, 1, *param_1 + param_4, buf);   // 真正的发送在等待之后
```

即：**发送之前先等一个"额度/锁"信号量（ctx+0x34），超时 100000 单位**；等不到就断言。
发送本体在等待之后（`thunk_FUN_0036c670` 进 SRAM 别名，参数里带 `"%02x "` 的十六进制转储格式）。

**ctx 是驱动自己的静态 PSRAM 对象 `0x3C448D94`**：它的地址在驱动自己的字面量池里出现
（file 0x4525BC 与 0x4525D4），信号量在 `0x3C448DC8`。同一驱动里还有第二处断言（行 432）。

**定性（等级：静态反编译 + 动态断言现场一致）**：这是 **vendor HCI 的发送额度等待**，
与"对端核应答"同类 —— BT 控制器那侧不给额度，AP 的 `bthci_send` 就等到超时然后断言。
下一步（已开跑）：`--wstop 3C448DC8 4` 盯这个信号量有没有人 post、是谁；
`--site 0x0C51274E 200` 盯断言调用点本身。

### 8.125 BT 对端其实已经建过一半：邮箱（0x400000A0/A4）与 TX_DONE 在 `mcu_mbox` 里

在怀疑"要不要新建 BT 对端"之前先核了现状，结论是**不该重造**：

- 参考实现 `_ref_miband_qemu/controllers/miband_bt.c:71-80,2029-2070` 定死了 AP↔BT 的握手：
  邮箱寄存器 `0x400000A0`（SET）/`0x400000A4`（CLR），每通道一位 `TX_DONE(ch)=1<<ch`，
  对端置起 TX_DONE 时**无条件**拉高对应中断线（`:1625-1649`）。
- 我方 `src/l2/dev/mcu_mbox.c` 已经按这套建模：从 AP 的发送环里取包、置 TX_DONE、
  按 profile 的 `bt.tx_irq=[32,33]` / `rx_irq=[34,35]` 拉线（见 profile 里 `mcu_mbox` 的
  `peer_ready=0x200FE2E2` 与 `ack` 指向 `mcu_mbox_rxdone` 那两段 provenance）。
- 所以 `bthci_send` 超时**不是"没建对端"**这么粗的解释；要么是这条发送没走邮箱（`bes_serial_hci2`
  名字里是 serial），要么它等的额度来自别的路径。nz2 的 `--wstop 3C448DC8 4` 就是为这个开的。

**纪律**：在 `--wstop` 给出"谁写/没人写"之前，不许拿"预置信号量非零"冒充修通 ——
那会把一个真实的握手缺口藏起来。

### 8.126 又一次踩了"数据断点要带起拍"这个坑（自己的记录没照做）

nz2 本来要盯 `0x3C448DC8`（BT 发送信号量），命令写成 `--wstop 3C448DC8 4`，**漏了第三参"第几条才武装"**。
结果 crt0 清 BSS 那一段（第 11,274,794 条，PC=0x0C0C0358、r3=0x3C448DCC）就把断点打中，整轮直接结束：
`执行 11274794 条后停机=没停(到上限)` —— 注意这个提示词是**误导**的，实际是断点命中提前收尾，不是到上限。

两条沉淀：
1. 任何打在 PSRAM/SRAM 缓冲区上的写断点，**必须**带起拍（本轮改 `--wstop 3C448DC8 4 3000000000`）；
2. 顺带确认了 `ctx=0x3C448D94` 在 **BSS 段**（crt0 会整段清零），所以它每次跑批都是干净初值 ——
   信号量计数若为 0，那是固件自己初始化成 0，不是残留。

### 8.127 run DZ2：断言点零命中，但抓到了"第一次睡"那一拍；启动形状第一次看得见

命令：`--deep 6000000000 --beat 1000000000 --site 0x0C51274E 200 --wstop 3C448DC8 4 3000000000`（`_probe/dz2.txt`）。

1. **`_assert` 站点 `0x0C51274E` 一次没命中**。这不是"BT 断言没了"的证据 —— 这一枪在 3,255,792,400
   条被自己的写断点停住了，断言本来要到超时那一拍才炸。run NZ 那条读数仍然有效，两条是不同的停法。
2. 命中的那一笔是**等待方自己**：`3C448DC8 <- FFFFFFFF (宽2) PC=0C4C6A62`。宽度 2、写进去 0xFFFF，
   就是信号量计数从 0 减到 -1（这一档 NuttX 的 `semcount` 是 16 位）。栈里同时有
   `3C448D94`(=ctx)、`0C512733`、`0C5125E9`(=bthci_send 的返回地址) ⇒ 确是 `bthci_send` 走进
   `nxsem_timedwait` 的第一拍。**定案：从开机到 3.2557G，没有任何人 post 过 `ctx+0x34`。**
3. uart1 攒到 3216 字节，尾部把启动形状讲清楚了（这段是固件自述，比任何推断都硬）：
   - BES 控制器初始化序列整段发出去了：`01 b8 fc` / `01 77 fc 08 …` / `01 01 10 00` /
     `01 fb fd 0c …` / `81 fc` / `86 fc` / `cf fc`(=0xFCFF) / `c3 fc` / `c4 fc` / `c5 fc` /
     `88 fc` / `82 fc` / `83 fc` / `84 fc` / `b3 fc` / `cd fc` / `32 fc` / `72 fc` / `71 fc` /
     `e5 fc` / `ff fc`×3。逐条与参考实现 `miband_bt.c:1158-1179` 的期望参数长对得上
     （FC77 长 8、FC81 长 8、FC82 长 9、FCFF ≥6）⇒ **包型口径没错，发得出去**。
   - 紧接着 `yaffs: 0 blocks to be sorted...`，然后 rcS 的任务表一条条打印：
     `offline_log[9] log_service[11] kvdbd[13] miwear_rtc_checker[14] chargerd[15] bluetoothd[19]
     miwear_bluetooth[25] nfc_stack_bridge[31] miconnect[35] miwear[52]` + `softbus_eng_init`
     `networking_eng_init`。**miwear 起来了，且这次没打印 PANIC** —— GPU 那一格把 §8.123 的炸点消掉了，
     这条与 run NZ 的结论一致。
   - 两行新的存储侧读数：`nsh: losetup: ioctl failed: 2`、`nsh: mount: mount failed: 15`。
     只登记现象，**还没归因**（这两条是"某次挂载没成"，不代表文件系统整条链死了 —— kvdbd/log_service
     都在它们之后起来了）。要往下追就先问"losetup 想挂哪个镜像、我们有没有铺"。

### 8.128 面板已经点亮，但一个像素都没送：两条新判据第一次有读数

`dsi 像素载荷：长包0 共0 字节（2C=0 3C=0）`，`dsi 写命令直方图（11 种）：
04x4 11x1 12x1 29x1 35x1 51x2 53x1 63x1 67x1 68x1 FEx1`。

- `0x11` = Sleep Out、`0x29` = Display ON 都发出去了 ⇒ **面板 init 走完、显示是开的**；
  `0x35` = tear-on、`0x51/0x53` 亮度与显示控制、`0x63/0x67/0x68/0x6E` 那一族厂商格也都写了。
- 长包 0、`2C/3C` 0 ⇒ §8.121 留的那个二分法有答案了：**既不是"没人踢 LCDC +0x224"也不是"命令模式面板
  在收帧"，而是渲染结果从头到尾没往面板送**。`lcdc 帧=0` 与 `dsi 像素载荷=0` 是同一件事的两个观测面。
- GPU 侧不是死的：`投活13 完成应答13 最近=384DF980 长度165`，命令缓冲前 16 字
  `31000B00 00000000 100C0C0C 1F191919 201A1A1A 20191919 30272727 3F343434 40343434 4F424242 …`
  —— 后面那一串 `(n<<24)|(v<<16)|(v<<8)|v` 且 v 随 n 单调升的是**灰度/伽马 ramp 表**，
  不是绘制命令。也就是说 GPU 在被真实编程（驱动在干活），但**没人把画好的帧交出去**。
- 顺带把 GPU 窗口的访问者坐实了：热表里 `40220004 首 PC=00390DA6`、`40220500/504 首 PC=00390DB6`
  的影子源是两条 2 字节跳板（见 §8.130）。

**一处自相矛盾的诊断文本，记下并要修**：同一份输出里 CPU 那行印
`SysTick … ctrl=00010003 load=00000010 val=00000000`（load 非 0），紧跟着一行却说
`SysTick 的 RELOAD 一直是 0（周期没被编程），因此按住的拍数=434`。两句互相打脸 ⇒
那句叙述不是从它自己印的那个字段来的。判据文本必须与它报的数同源，否则这种"看着像结论"的句子
会把人带偏（同一条纪律见 AGENTS 陷阱 43）。

### 8.129 BT 那头的真根因：模型根本没读到 AP 发的包（不是额度协议没建）

`mcu_mbox` 的账：`门铃40 次 回包1 投递1（共7 字节）｜发完中断40`，
`AP 发送队列 200C7914（步长 4）队列读25,436,155 命令0 空25,435,918`，
`取包判定链（最后 16 笔）` 全是 `00000000/00000000/3`（动作 3 = 头格与尾数组两头都空）。

⇒ 2500 万次读那一格恒 0，bt_ram+0x4000 那个头格也恒 0，而固件明明把 40 包发出去了（uart1 里看得见字节）。
**所以"BT vendor HCI 发送额度对端没建"这个说法要改成：对端在读错的地方。**
`回包1` 那一包是开机的 `{04,05,04,00,03,0C,00}`（AP 的分派器咬的就是这一形），
之后每一包 Command Complete 都是"没读到真命令就不凭空造包"那条规则挡住的 —— 规则本身是对的，
它只是再也没等到命令。

`tx/head_addr=0x200C7914` 的来源记的是"发送函数 0x0C2314DC 的字面量"。本轮核过：
file 0x1715CC（=0x0C2315CC 那一支）与真正在执行的镜像代码字节**不同**，且执行中的那支持有
INTERSYS 的字符串（§8.130）⇒ **那一档很可能是另一台 mailbox（sensor 核那条链），不是 BT HCI 这条**。
这条不改结论只降等级：`0x200C7914` 从"AP 发送队列头"降为"待证，可能不属于 HCI 这条链"。

### 8.130 口径纠正：`0x003xxxxx` 是 RAMX 别名指向 SRAM，不是 flash 镜像；换算器那一档解错了

1. `best1502_r0.soc.json` 自己就写了 `sram: base 0x20000000, aliases [0x00200000], alias_kinds [ramx]`
   ⇒ delta **0x1FE00000**。所以热表里 `PC=003915CC / 003914A0 / 00390DA6 / 00390DB6` 是
   SRAM `0x201915CC / 0x201914A0 / 0x20190DA6 / 0x20190DB6` 的执行别名，**不是**"减 0x363AE8 得 file 偏移"。
2. `flint-vm/tools/addr_conv.py which 0x003915CC` 答"按 SRAM 执行别名 解 -> file 0x0002DAE4" ——
   那是拿 XIP 口径去减出来的**一个看着合理的错地址**（同批里 `0x00390DA6` 解成 `0x2D2BE`，
   此前已实测那一段解不出合法指令）。**根因**：这档输出假装给了 file 偏移，但它不可能知道倒灌的
   落点；正确做法只有一步 —— 拿 SRAM 运行地址去 dump 里取字节，再在 `vela_ap.bin` 里搜（唯一命中才算数）。
   已把这一档改成"RAMX 别名 → SRAM 运行地址 + 必须走影子源字节搜索"，不再吐 file 偏移。
3. 影子源（`_probe/ramx_shadow.py`，32 字节在 11,793,540 字节的镜像里**唯一命中**）：

   | RAMX 别名 | SRAM 运行地址 | 影子源 file | 那段是什么 |
   |---|---|---|---|
   | 0x003915CC | 0x201915CC | 0x0002B5E4 | 写 CMU+0xA0/+0xA4 的那支（邮箱初始化） |
   | 0x003914A0 | 0x201914A0 | 0x0002B4B8 | `hal_intersys_tx_irq_handler`（链表分派） |
   | 0x00390DA6 | 0x20190DA6 | 0x0002ADBE | `c058 7047` = `ldr r0,[r0,#8]; bx lr`（寄存器**读**跳板） |
   | 0x00390DB6 | 0x20190DB6 | 0x0002ADCE | `c150 7047` = `str r1,[r0,#8]; bx lr`（寄存器**写**跳板） |

   GPU 那一栈的两次访问各走这两条跳板，与"UI 任务在 file 0x2AE3A 那一支"（§8.112 起）是同一段代码。
4. INTERSYS 的表结构（`_probe/mbox_literals.py` 直接读 `0x2B494..0x2B4B0` 的字面量池；
   等级 = 固件静态，运行地址由 dump 逐字节核过）：
   - 链表头数组 **SRAM 0x2019D990**（每通道 4 字节，`head[ch]`）；通道上下文数组 **0x2019D998**（步长 0x3C）；
   - 收/发处理函数表 **PSRAM 0x3C4911D8**（`ldr pc?,[tab, type*4]`）；`type = *(字节*)(节点+4)`，
     只允许 0/1，越界就打印 `INTERSYS-TX: Invalid msg type: %d` 并 `_assert(…, 0x25f)`；
   - 另外 0x3C4911D0 / 0x3C4911C0 / 0x3C4911A0 三张数组在同一段初始化里被清 0 与互相挂钩。
   - 字符串证据：`file 0x00313208` = `INTERSYS-TX: Invalid msg type: %d`，
     `file 0x003132AA` = `hal_intersys_tx_irq_handler`（PSRAM 串池换算照旧 `VA-0x3C000000+0x2FDC4`）。
5. **数值撞车的提醒**：本文 §8.62/§8.64 的"fb ioctl 0xFB / 0xFD"与本轮 uart1 里的 HCI 厂商命令
   `opcode 0xFDFB`、事件包首字节 `0x04` 是**两套不相干的东西**，别再互相引用。

### 8.131 `FUN_0044f63c` 是什么终于读出来了：它是**面板命令分派器**，不是 ioctl 处理

Ghidra 反编译 file 0x44F63C（`_probe/fb_ioctl.txt`）。`param_2` 是命令描述符，字段用得很清楚：

| 描述符偏移 | 含义（反编译里怎么用的） |
|---|---|
| +1 | 命令字节 `bVar3`，分派就按它 |
| +2 | 标志 `bVar2`：bit1 决定走"厂商命令"档（`(bVar2<<0x1e)<0`），bit2 决定读还是写 |
| +4 | 长度（写回返回值） |
| +8 | 载荷指针 |
| +0xc | 数据长 |

认得的命令字节分两档：
- 普通档 `{0x05, 0x06, 0x15, 0x39}` —— 就是 MIPI DDI opcode（写短包无参 / 读回 1 参 /
  写短包 1 参 / 写长包）。这一档走 `FUN_008ada0c` / `FUN_008ad7e0` 真发总线包，
  与我们 DSI 模型看到的 `0x15`/`0x39` 两支对上。
- 厂商档 `{0xF0, 0xFB, 0xFC, 0xFD, 0xFF}` —— **一个总线包都不发**，直接调引擎：
  `0xF0→FUN_008a8a10`、`0xFB→FUN_008a89cc`（帧请求）、`0xFC→FUN_008a8a30`、
  `0xFD→FUN_008a8a64`（搬帧）、`0xFF→FUN_008a8a20`。

**要显式改掉的旧说法**（纪律 18）：§8.62/§8.64 一直写"显示服务对设备发 **ioctl** 0xFB/0xFD"。
0xFB/0xFD 的对应关系（前者=帧请求、后者=搬帧）是对的，但它们**不是 ioctl 号**，
是这台分派器的厂商命令字节。正确的问法是"**谁给面板发厂商命令 0xFD**"。

这台分派器怎么被叫到，静态到此为止：
- 它在整个镜像里**没有任何引用者**（Ghidra "引用这个函数的地方" 0 条）；
- 它的 Thumb 指针 `0x0C50F63D` 全镜像只出现在 **file 0x3E9544** 一格，那是一张三格表：
  `0x3E953C→file 0x44F5FD`、`0x3E9540→file 0x44F615`、`0x3E9544→file 0x44F63C`；
- 表基址 `0x0C4A953C/40/44`（含 0x2C 缓存别名）**在镜像里一次都没有被引用**
  ⇒ 表是运行期被装进某个对象的，指针由计算得到，静态扫字面量这条路到此为止。
  下一枪用 `--rstop 0C4A9544 4` 在"读那一格"的瞬间停，直接拿调用者 PC（run EA `fb_tab`）。

顺带把帧请求的**实测调用者**定名了：ea1 那一次命中的 `lr=0C95055B` 落在
`FUN_00890470`（file 0x890470）。反汇编是一段真正的搬图例程：
`ldrd r0,r2,[r5,#4]` + `lsl #2` 查表（调色板）、`ubfx r2,r2,#5,#0xb` 取 11 位宽、
`mla r12,lr,r1,r2` 算行偏移。⇒ **"UI 在画、并把刷新区域登记进去了"这条现在有直接证据**，
缺的只是后面那一跳。

### 8.132 run EA2 把 BT 那道的真根因钉死，并且改对了 profile 的一格

命令：`--deep 6000000000 --beat 2000000000 --wstop 2019D990 8 2000000000`（`_probe/ea2.txt`）。

1. **命中**：`第 2905765184 条 2019D990 <- 2019D998 (宽4) PC=003918D2`。
   `PC=003918D2` 是 RAMX 别名 → SRAM `0x201918D2` → 影子源 **file 0x2B8EA**（32 字节唯一命中）。
   这一拍与模型自己记的"最后一次门铃 45,402,581 拍 ×64 ≈ 2.9G"**同一拍** ⇒ 阳性对照成立：
   这笔写就是发一包时的挂链，不是别的东西。
2. **写进去的值是 `0x2019D998`** —— 就是通道上下文块本身。也就是说 INTERSYS 每通道用
   一个**静态节点**（块基 0x2019D998，步长 0x3C），发的时候把它挂进 `head[ch] = 0x2019D990 + 4*ch`，
   然后按 `0x400000A0 = 4`（DATA_IND）。这与反编译里 `*piVar1 = *piVar3; dsb; _DAT_400000a0 = 4` 逐字对上。
3. **节点五个字再独立核了一遍**（不靠反编译，直接读 SRAM dump 现值）：

   | 通道 | 块地址 | +0 next | +4 state | +8 len | +0xC ptr |
   |---|---|---|---|---|---|
   | 0 | 0x2019D998 | 0 | 0 | **0x5E**(94) | 0x2019D574 |
   | 1 | 0x2019D9D4 | 0 | 0 | **8** | 0x2019D56C |

   与 profile 现有的 `rx/node/{next=0,state=4,len=8,ptr=0xC}` 完全一致 ⇒
   **只需要改头格地址这一格，节点的读法本来就是对的。**
4. 已改 `best1502_r0.soc.json` 的 `mcu_mbox.tx.head_addr`：`0x200C7914` → **`0x2019D990`**，
   重生成 `profile_p65.prf` 后 `mbox.tx.head.base 0x2019d990` 在生成物里核到。
   `test_dsi 111 / test_lpu 39 / test_gpu 45 / test_mcu_mbox 211` 四项在新 profile 下仍全绿。
5. **一处自我更正（纪律 18）**：profile 里那条"发送函数 0x0C2314DC 的字面量 0x200C7914"
   不是 HCI 这条链的头格 —— 那是**另一支代码**（file 0x1715xx 那一份）的池，它的字节与
   SRAM 里真正执行的镜像副本不同（逐字节比过）。所以旧地址"25M 次读恒 0"不是巧合，是读错了对象。
   尾数组那一格（0x200FEABC）**仍未核实**：AP 的待发指针是 `0x3C4911D0` 里的**间接**一格，
   模型现在表达不了二次解引用，先留着（读不到只影响 promote 之前那一窗口的可见性）。
6. 下一枪（run FC1，`--site 0x0C51274E 200`）的判据是**可分辨的三档**：
   `读到命令 N>0` ⇒ 模型第一次读走真命令；`回包 N>1` ⇒ 开始答 Command Complete；
   跑到 3.26G 之后 `ctx+0x34` 不再出现"减到 -1"那一笔、并且 6G 内不打 `_assert(299)`
   ⇒ 额度这一环真的通了。反过来，如果 `命令` 仍 0，就说明挂链发生在别的通道/别的格，
   这一档要重来而不是继续加判据。

### 8.133 run FB-TAB（旧 profile 跑到 6G 的对照）：末态是一条自环，且 5.0576G 之后没有任何调用

命令：`--deep 6000000000 --beat 2000000000 --site 0x0C968A64 200 --rstop 0C4A9544 4 200000000`
（`_probe/fb_tab.txt`；这份跑的是改 profile **之前**的镜像，正好当对照）。

1. **末态**：`执行 6000000000 条 … 指令地址=0036EB36，编码=0000E7FE` —— `0xE7FE` 就是 `b .`。
   `PC=0x0036EB36` 是 RAMX 别名 → SRAM `0x2016EB36` → 影子源 **file 0x0000B04E**（32 字节唯一命中），
   进来之前那一笔是 `bl @0036E5A2 -> 0036F4D2`（影子源 file 0x0000AABA / 0x0000B9EA）。
   `BASEPRI=80`、`PRIMASK=0`、`IPSR=0` ⇒ 不是关中断死等，是**主动收尾的自环**。
   这与 run NZ 的"PANIC → board_reset → 收尾自环"是同一条尾巴的形状。
2. **最后一次调用记录在第 5,057,610,302 步**，之后到 6G 全是那条 `b .`。
   这一拍正是 `bthci_send` 那次 `nxsem_timedwait(ctx+0x34, 0x186A0)` 该到期的地方：
   DZ2 抓到"开始睡"在 3,255,792,400，两者差 1.8019G 条 —— 与 100000 拍的超时窗口同量级。
   ⇒ **5.06G 这一刀就是 BT 那道门**，不是显示链。显示链的问题在它之前就已经看得见（下面第 3 条）。
3. 六项读数（旧 profile 的 6G 基线，后面每改一枪都跟这组比）：
   - `gpu … 投活37 完成应答37 最近=384DF980 长度165` —— 从 DZ2 的 13 笔涨到 37 笔，UI 一直在提交；
   - `dsi 像素载荷：长包0 共0 字节（2C=0 3C=0）` —— 60 设备秒内一个像素都没送；
   - `mcu_mbox … 门铃42 回包1 投递1` —— 对端只答过开机那一包；
   - `lcdc 帧=0 中断=0`、`te 给=92 跳=4` —— TE 与 GPIO 中断一路在给，LCDC 始终没起帧；
   - uart1 停在 3216 字节，最后一行是 `miwear [52:104]`，**它之后一行都没有**；
   - `非对齐 16646`、最后一次坏访问 `201969B8 unaligned` —— 与显示链无关，登记备查。
4. **`--rstop 0C4A9544 4` 零命中不能当"没人调用分派器"的证据**（自己拦自己一次）：
   那一格是 flash 里的三格 ops 表（attach/detach/dispatch），NuttX 注册驱动时通常会把 ops
   **拷进 file/inode 结构**，运行期读的是 RAM 里那份副本，打在 flash 原件上的读断点本来就看不见。
   所以本轮只能得到"这一枪看不到"，得不到"没发生"。要问同一件事得改站点：
   直接挂分派器入口 `0x0C50F63C`，或挂 `搬帧 0x0C968A64`（这一条挂了，6G 零命中——
   这一条是有效的阴性，因为它打在代码上，不是打在可能被复制的数据上）。

### 8.134 本轮未决问题（写清楚，别让下一轮重新发现）

1. **尾数组仍未核实**。`mbox.tx.tail.base = 0x200FEABC` 是从旧那支代码（file 0x1715xx）的池抄的；
   INTERSYS 这一支的"待发节点"是 `0x3C4911D0` 里存的**指针所指向**的那一格（反编译里
   `piVar1 = 0x2019D990; piVar3 = 0x3C4911D0; *piVar1 = *piVar3`），要多解一次引用。
   模型现在的形状只能"基址 + 通道×4"直接读，表达不了二次解引用。
   后果局限在"AP 还没 promote 的那个窗口里那一条命令要等一拍"，不是死结 —— 但 FC77 恰好落在
   哪一拍还没核，所以额度那条链如果仍不通，第一个要怀疑的就是这里。
2. **两处结论不一致，登记待解**：profile 与旧文档把 `0x0C229708` 叫"HCI 事件分派器"
   （判据 `[0]=4/[1]=事件码/[2]=参数长/[3]=0`），但 Ghidra 说这个地址落在
   `FUN_001696c4` 里，而那一段满屏是 `udiv`/`vcvt.f32.s32`/`vsub.f32`/乘 `0x70000` 的缩放循环，
   看着是图形/标度例程不是包解析。按纪律 17，两个结论都不能直接引用。
   **不受影响的是行为侧证据**：我方那一包 `{04,05,04,00,03,0C,00}` 确实被 AP 收下并回显、
   对端就绪位也被写了 —— 那是在 run 里读到的，不是靠这个名字推的。
3. **存储侧两行没归因**：`nsh: losetup: ioctl failed: 2`、`nsh: mount: mount failed: 15`。
   要问的是"rcS 里 losetup 想挂哪个镜像、我方 NAND/NOR 镜像有没有铺到那一格"，
   不是先改判据。`kvdbd`/`log_service` 在它们之后照常起来 ⇒ 这条目前不挡主链。
4. **SysTick 那条自相矛盾的诊断文本改了实现，但还没重编验证**：改成
   "有 N 拍是在 RELOAD=0 期间按住的（本期编程过的最大 RELOAD=%08X）"，
   并把 `st_no_reload` 补进 `fl_scb_reset` 的清零名单（原来跨 board_reset 会累加）。
   热表里 `+014 值=0x10` 与"按住 434 拍"两个数要在下一次全量 build.sh 之后重新对一遍。
5. **`--rstop` 打 flash 里的 ops 表看不到调用者**（§8.133 第 4 条）。同一问句的正确挂法是
   代码入口：分派器 `0x0C50F63C`、搬帧 `0x0C968A64`。下一枪把分派器入口补上，
   预算给足（多站点共用一个全局计数，任一站点用满就把所有站点一起下线）。

### 8.135 run FC1：**BT 那道门过了** —— 只改 profile 一格，对端第一次真正答上话

同一份 `--deep 6000000000 --beat 2000000000 --site 0x0C51274E 200`，唯一变化是
`mbox.tx.head_addr` 从 `0x200C7914` 改成 INTERSYS 的链头 `0x2019D990`（`_probe/fc1.txt`）。

| 读数 | 旧 profile（FB-TAB，6G） | 新 profile（FC1，6G） |
|---|---|---|
| 门铃 / 回包 / 投递 | 42 / **1** / 1（7 字节） | 99 / **75** / 75（525 字节） |
| AP 发送队列读到命令 | **0**（25,436,155 次读全空） | **102**（队列读 46,875,779，非命令 0、坏 0） |
| `_assert` 站点 `0x0C51274E` | 5.06G 进收尾自环 | **零命中** |
| 6G 末态 | PC=0036EB36 编码 `E7FE`（`b .`） | PC=0C974662 编码 `4770`（`bx lr`） |
| 异常 进入/返回 | 41,308（5.06G 后冻住） | **52,000 / 52,000，还在涨** |
| TE / GPIO 中断 | 92 | **140** |
| GPU 投活 / 应答 | 37 / 37 | **49 / 49** |
| uart1 | 3216 字节 | 3304 字节 |

⇒ **系统在 60 设备秒这一档是活的**，这是本轮唯一真正想要的变化：判据不是"多跑了几条"，
而是"该炸的那一枪没炸、末态不是自环、中断与 TE 还在进"。

- 最后八条被读到的 opcode：`0C24 0C24 0C19 2008 0C24 0C3A 2009 0C3A` —— 全是标准 HCI
  （OGF 0x03 链路策略 / OGF 0x04 LE 那族）。对照 FB-TAB 时代"命令 0 条"，
  现在 BT 栈在往下走自己的 GAP/LE 初始化，而不是发完就睡死。
- **两处吞吐缺陷，登记并且会成为深跑的新瓶颈**：
  ① `丢弃 28` —— 待答队列只有 8 格，命令来得比对端答得快就溢出。
    每一条丢弃在 AP 那侧就是一次 `100000` 拍超时，28 条 ⇒ 累计的超时窗口比省下的时间贵得多；
  ② `到期但槽占着 9,203,496 次` —— 一个投递窗口只摆得出一包，命令答完之前后面全堵着。
  两条都是"我方对端太慢"，不是协议判据错；下一枪先看 24G 深跑里 BT 有没有再犯，
  犯了就按这两条改容量与投递节拍（改 `src/` 就要再全量回归）。
- 首帧侧仍未过：`dsi 像素载荷：长包0 共0 字节`、`lcdc 帧=0`、`搬帧 0x0C968A64` 站点零命中。
  但 GPU 那边在画真东西：命令缓冲换成 50 字节那一支，前 16 字
  `30010A10 00000109 … FF222222 … 3F800000` —— `0x222222` 是深灰底、`3F800000` 是 `1.0f`，
  是 UI 状态更新而不只是那张伽马 ramp。

### 8.136 首帧的真上游可能根本不在显示链：rcS 那八个 `losetup` 全在打不开 `/system/*.bin`

读 rcS 原文（`_probe/rcS_text.txt`，早前从镜像里取的那一份），显示服务起来之前有一段：

```
losetup -r /dev/loop0 /system/app.bin        ; mount -t romfs /dev/loop0 /resource/app
losetup -r /dev/loop1 /system/watchface.bin  ; mount -t romfs /dev/loop1 /watchface
losetup -r /dev/loop2 /system/font.bin       ; mount -t romfs /dev/loop2 /font
losetup -r /dev/loop3 /system/i18n.bin       ; mount -t romfs /dev/loop3 /i18n
losetup -r /dev/loop4 /system/misc.bin       ; mount -t romfs /dev/loop4 /resource/misc
losetup -r /dev/loop5 /system/system.bin     ; mount -t romfs /dev/loop5 /resource/system
losetup -r /dev/loop6 /system/vendor.bin     ; mount -t romfs /dev/loop6 /vendor
losetup -r /dev/loop7 /system/quickapp.bin   ; mount -t romfs /dev/loop7 /quickapp
```

**`/resource/app` 挂不上就没有 launcher/应用**，也就没有人去发那一记搬帧。这条比
"厂商命令 0xFD 没人发"更接近上游，而且是**我方镜像内容**的问题，不是模型缺件。

两条已有证据，注意它们不是同一个失败模式：

1. 上一轮的全量日志收割（`_probe/all8_lo.txt`，从 SRAM 日志缓冲 `0x200DDE50` 起）记下
   **八个文件全部** `losetup: ERROR: Failed to open /system/xxx.bin: -13` —— 文件名认得了、
   open 被拒。`-13` 是 EACCES，与 `gen_yaffs_image.py` 顶部那条定案完全对得上：
   **`yst_mode` 摆错/为 0 的后果就是 open 一律 EACCES**（name 结束在 266、要 2 字节填充，
   mode 这个 u32 在 **+268** 而不是 266）。
2. 本轮 FC1 的串口头 3304 字节里只有一条 `nsh: losetup: ioctl failed: 2` 与一条
   `mount failed: 15`。**这条计数不能当"只有 loop0 失败"** —— p65 走延迟日志、
   模型只留了缓冲头部，八个失败很可能被截掉了。要拿全量必须挂日志包装函数收。

一条本轮走过的死路，记下来省时间：想直接在 `fw/p65_100_043/vela_resource.bin` 里按
`name+258` 读 `yst_mode` 是**读不出来的** —— 实测 `app.bin` 那两处命中的
`type/parent` 全是 `0/2585722880`、`583863364/1633912091` 这种垃圾，说明那两处只是
**字符串**（romfs 或资源表里的名字），不是 yaffs 对象头。要按 yaffs 口径定位对象头，
必须先 `--nanddump` 把模型真正呈现给固件的页取回来，再按"页 2048 主区 + 64 备用区、
对象头在 `chunk_id=0` 那一页"去对 —— 不是在这个文件里平铺着搜。

**下一步的次序（别倒过来做）**：先全量收日志把"几个失败、errno 各是多少"钉住 →
`--nanddump` 定位 `/system` 里 app.bin 的对象头读它自己的 mode → 需要铺就照
`gen_yaffs_image.py --header-from`（用固件自己写下去的那一页当模板，这是本文件里已定案
"目标固件只认这一条"的做法）产出到 `flint-vm/build/` 下的**副本**并让 board 配置指过去，
**不改 `fw/` 里的原始固件** → 判据可分辨：八条 losetup 错误清零 + `/resource/app` 挂上 +
launcher 出现在 rcS 打印里 + `dsi 像素载荷` 或 `lcdc 帧` 第一次非零。
把 `/dev/loop` 或 `open` 桩成成功是冒充通过，禁止。

### 8.137 端口主线回到台前：批量锚点只能从"固件自己填进 RAM 的表"拿

用户要求把 Chaos 真正移植到 RW6，所以这一节开始记录**端口侧**的取证，不再只跟显示链。

先立两条已实测的"此路不通"，免得下一轮重复试：

1. **043→p65 拿函数序言字节对拷不成立**：`obj_set_width`(file 0x004C8420) 与
   `set_text`(file 0x004C9490) 取 8/16 字节在 p65 全镜像 **0 命中**；同一段在 043 自己里
   16 字节唯一命中 —— 阳性对照做过，所以这个阴性算数（两版 LVGL 都在，但不是同一次编译）。
2. **按名字找 C 函数在 p65 上也走不通**：LVGL 核心 API 的名字后缀在 p65 镜像里**一个都没有**
   （`set_width`/`set_height`/`set_pos`/`set_style_prop`/`obj_create` 全 0 次），
   `align`/`delete`/`set_text` 那几处命中属于 JS 对象原型表（Date/Map 那一族），不是 LVGL。
   调试名字池整片无静态引用（`lvx_zoom_picker_set_mode`、`miwear_timer_create` 都 0 引用）。

于是唯一能"一次拿一批地址"的入口是**运行期表**：固件跑起来之后会自己把绑定表填进 PSRAM，
记录形如 `{名字槽 VA, 函数指针}`，同表内槽距固定。做法（`scripts/scan_fw_ram_tables.py`，
明细落 `docs/fw_runtime_tables_p65.csv` 160 行）：

```
./flint-vm/build/test_boot.exe flint-vm/build/profile_p65.prf --deep 2600000000 \
    --ramdump 3C000000 600000 _probe/p65_ps6m.bin
python scripts/scan_fw_ram_tables.py _probe/p65_ps6m.bin 3C000000
```

判据与防伪：原始配对 1540 条，但单条基本是巧合（实测 `ECDSA` 与 `EC_DH` 指向同一地址），
所以只认**同组槽距一致且 ≥3 条**的，收到 40 张表。这批地址的来源等级写清楚：
**地址 = 固件自己在模拟里填进 RAM 的值（SIM_CONFIRMED），名字归属 = 表里的串（静态），
函数语义 = 还没核**。三者不能混着报。

拿到的第一批就是端口最需要的**页面生命周期**（file 偏移，运行地址 = +0x0C0C0000）：

| 名字 | p65 file | 运行地址 |
|---|---|---|
| `initPage` | 0x009183E9 | 0x0C9D33E9 |
| `showPage` | 0x00918349 | 0x0C9D3349 |
| `hidePage` | 0x009184AD | 0x0C9D34AD |
| `destroyPage` | 0x00918A5D | 0x0C9D3A5D |
| `dumpPage` | 0x009182AD | 0x0C9D32AD |
| `getI18NEntries` | 0x00918221 | 0x0C9D3221 |
| `loadI18N` | 0x00918151 | 0x0C9D3151 |
| `createVdom` | 0x00917FE5 | 0x0C9D2FE5 |
| `createElement` | 0x00919799 | 0x0C9D4799 |
| `runGC` | 0x00918A09 | 0x0C9D3A09 |
| `setmode` / `getmode` / `keepscreenon` | 0x0090FC05 / 0x0090FD95 / 0x0090FCCD | — |
| `scroll_to` / `scroll_by` / `scrollbar_invalidate` / `scrollbar_mode` | 0x00991F0D / 0x00991781 / 0x00992455 / 0x0098E819 | — |

另有一张 widget 类表（Label/List/Checkbox/Image/Dropdown/Keyboard/Led/Calendar/Font/Anim/Arrow，
函数集中在 file 0x00995Dxx~0x009965xx）——这一带就是 lvx 控件层。

**下一步的走法（已定，不再犹豫）**：这些表里的函数多半是 JS 绑定壳（参数是 argc/argv），
Chaos 是原生模块要调 C 实现，所以把它们当**跳板**：反编译壳函数 → 取它 `bl` 进去的被调函数 →
与 043 已知 API 的结构对照（参数个数、临界区、对 `+0x148` 那类字段的操作）→ 候选落成
`fw_api` 的 p65 地址 → 每条都在模拟器里挂站点确认真的被执行。
第一批跳板：`initPage`/`showPage`/`hidePage`/`destroyPage`/`createElement`。

### 8.138 一次假阴性撤销：p65 的串**是有引用的**，只是引用值在 PSRAM 的 0x3C 域

此前多处记录（含 skill 与 AGENTS）写着"p65 的字符串/名字池**无静态引用**"。
**这条是错的，根因是只搜了三个 flash 别名**（0x0C / 0x2C / 0x28），而 p65 在
file 0x1C0000..0x3C0000 这一带是**倒灌进 PSRAM 的 blob**，代码里的串指针用的是
`VA = file + 0x3C000000 - 0x2FDC4`（与串池口径同一个式子）。

按 0x3C 域重搜，引用立刻出现，且大多**唯一命中**：

| 串（来自 043 已定位函数体内） | p65 串位置 | 按 0x3C VA 搜到的引用点 |
|---|---|---|
| `[%s] %s: [%s] installation failed`（043 `app_install` 体内） | file 0x003808D0 | **1 处** file 0x00181A6C |
| `app_launcher_add` | file 0x0026E5B4 | 1 处 file 0x0072C530 |
| `initPage` | file 0x0031E6A0 | 1 处 file 0x0031E7EC（表内） |

⇒ 这条撤销直接改变移植的成本估计：**"跨镜像对拷锚点"不是没有键，而是键要用 PSRAM VA 形态去搜。**

### 8.139 端口量的三件工具与第一版绑定结果（附一处自陷）

1. `scripts/p65_anchor_coverage.py` —— 先把成本量清楚：
   Chaos 侧共 **99 处** `transmute(0x0C..)`；043 官方符号包只覆盖 **38 处**；
   两版镜像**同名**的框架函数却有 **461 条**（`page_` 84 条、`lvx_` 113 条、`miwear_` 210 条…），
   但 LVGL 核心 API 的名字串两边都没有（`lv_obj_align`/`lv_label_set_text` 在 **043 里也没有**）
   ⇒ 043 那批本来就是靠结构恢复的，p65 不能指望名字直查。
2. `scripts/p65_anchor_map.py` —— 用"043 函数体内的串键 → p65 里按 PSRAM VA 搜引用"批量绑。
   第一版（取体窗口 0x500）出来 25 处绑定，看着很好，但**三处不同锚点投到同一个 p65 邻域**
   （`gfx/lvx_object_align`、`glass/lv_obj_move_to_index`、`page/lvx_content_pad_bottom`
   都落在 0x0C876EDC，键是 `[%s] %s: there no one use mana…`）—— 那是窗口越进邻居函数
   把别人的串当了自己的键。**加了两条纪律**：
   ① 判据从"某枚键唯一命中"改成**至少两枚不同的键投到同一 ±0x400 邻域**（多键投票）；
   ② 撞车检测：多支锚点投到同一邻域就整批降级为"待硬核"，不留在可交付里。
   第二版把窗口收到 0x80 反而只剩 1 处 —— 说明**必须有真实的函数边界**，不能靠固定窗口猜。
   ⇒ 正在跑 043 的 Ghidra 全量分析，拿函数边界后再出第三版（工具与判据都已就位）。
3. 目前**没有任何一处绑定达到"定案"**。定案要求三件齐全：
   Ghidra 给出 p65 函数入口 + 参数个数/临界区与 043 那支对得上 + 模拟器里挂站点看到真的执行。
   已到手且较可信的候选（多键投票 3~8 键同意，且键本身就是同名函数串）：

   | Chaos 侧 043 锚点 | p65 候选 run | 投票键数 | 依据 |
   |---|---|---|---|
   | `app_launcher_add` 0x0C513A44 | 0x0C7EC530 | 4 | 串 `app_launcher_add` 同名 + 同域另键 |
   | `app_launcher_del` 0x0C513B2C | 0x0C7EC5C0 | 3 | 同上 |
   | `lvx_page_title_create` 0x0C4CA6C4 | 0x0C21B4A4 | 3 | 同名串 + `text is NULL, create…` |
   | `page_goto` 0x0CA6C358 | 0x0C222E30 | 3 | `goto_page_no_check_stacktop`、`activitymanager_kill_bg_apps` |
   | `lvx_list_item_update` 0x0C4C8904 | 0x0C2198AC | 3 | 同名串 + `no obj,set icon error` |
   | zoom_picker `set_mode`/`set_options` | 0x0C2405D8 / 0x0C240850 | 3 | 同名串成组 |
   | 亮度族（043 4 支） | 0x0CA658A8 / 0x0CA65A44 / 0x0CA65E94 / 0x0CA65554 | 4~8 | `apply_brightness_now` 等成组 |
   | 表盘管理器 0x0CA95C30 | 0x0CA90B04 | 6 | `watchface_aod_destroy` 等同域 |

   注册链（`app_launcher_*`）与页面链（`page_goto`/`lvx_page_title_create`）都在里面 ——
   这两批正是"Chaos 能不能在 RW6 上装进去、注册出页面"的关键。
