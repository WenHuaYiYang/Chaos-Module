---
name: p65-chaos-page-port
description: 给 Redmi Watch 6（p65 / NuttX）上的 Chaos 加一个页面的固定流程：找固件锚点 -> 在 MiBand Studio 的 RW6 guest 里做执行级验证 -> 页面落地 -> 回归验收 -> 出安装包。每加一个页面都按这条链走，禁止只做静态推断就发版。也用于修 p65 模块的界面/取数问题。触发词：RW6 移植、p65 加页、Chaos 移植到手表、RW6 页面、亮度页、缓存清理页、执行验证、MiBand Studio guest、p65 安装包。
---

# p65（Redmi Watch 6）上给 Chaos 加页面

RW6 是 NuttX + Vela 固件，跟 043（10 Pro）**完全不是一套**：
没有表盘容器、没有 043 的 fw_api 地址表、没有固件自带的 Lua 安装器。
RW6 上的 Chaos 是一个 **NuttX modlib 内核模块**（`work/p65_chaos`），
靠 `insmod` 装载、靠固件的 app 框架注册成应用。

## 铁律

1. **每个锚点都要执行级验证**，判据是"读回/现象变了"，不是"没崩"。
2. **回调只登记，动作放拍里**。点击回调里不许做文件 IO、不许发触觉、不许改对象树。
3. **一拍只做一个 VFS 动作**。整棵目录树删完会把 UI 线程占死，表现为"应用退出"。
4. **屏幕睡着时 LVGL 定时器不推进**：装完模块先按表冠唤醒，否则 tick 只来第一拍，
   看起来像"代码没生效"。
5. **验界面前必须整机重启**：旧模块还挂在系统里时会占着 APP_ID，
   新模块 register 成功但屏幕上是**旧模块**的页面。
6. **旧探针模块会抢答**：不同名的旧模块照样跑各自的 tick，
   单次锚点验证前先 `rmmod` 掉，否则会把"成功"读成 ENOENT。

## 一、找锚点

p65 的固件带 LVGL 调试串（`__func__`），用现成脚本反查：

```bash
python scripts/p65_nameref.py --names brightness     # -> docs/p65_nameref_brightness.csv
```

脚本给的是 **file 偏移**：运行地址 = file + 0x0C0C0000（flash XIP）
或 file + 0x1BFD023C（PSRAM 里的 VFS）。当函数指针用要 `|1`。

**地址一律用脚本现算，不许心算**：手算 `0x9B37A8 + 0x0C0C0000` 写成 `0x0CA637A8`
（少 0x10000）会让调用跳进指令中间 = guest 直接 503，看起来像"这个 API 不能调"。

```bash
python -c "print(hex(0x9B37A8 + 0x0C0C0000))"
```

看结构用 Ghidra：`python scripts/ghidra_fw.py fns firmware-watch6-p65-100043 0x9A5C48`；
找调用点可以自己扫 Thumb BL（32 位编码：h1 的 11110 + h2 的 11D 段，
目标 = PC+4 + S<<24 | I1<<23 | I2<<22 | imm10<<12 | imm11<<1）。

**固件把全局块指针藏在重定位字面量里时**：固件自己 `ldr r3,[pc,#N]` 取的那个位置，
运行期就是指针；读同一个位置即可，别去猜地址（见亮度状态块）。

## 二、执行级验证

锚点地址本身按 `p65-anchor-exec-probe` 那套流程验证；**页面级的验证**这样做：

1. 在模块里加一个只做一件事的探针（feature 开关隔离），结果写 `DIAG[]`；
2. `bash work/p65_chaos/build.sh <feature>` → 补 e_entry → 上传 → `insmod`；
3. **现场要由 NSH 造**（`echo hello > /data/x` / `mkdir`），不要指望模块自己造；
4. 读回：QMP 读模块 DATA 段里的 `DIAG`（先 `lsmod` 拿 DATA 基址，按魔数 0xC0DE0034 定位）；
5. **判据优先用"现象变了"**：文件没了、页面读数变了、状态块字段变了。

已经踩过的具体坑（别再踩）：

| 现象 | 真因 |
|---|---|
| unlink 返回 -1，但文件真没了 | 另一个旧探针模块先删了，这个撞上 ENOENT |
| 新代码不生效，页面还是旧的 | 旧模块占着 APP_ID，必须整机重启 |
| tick 只跑一拍就停 | 屏幕睡着，LVGL 定时器不推进；先按表冠 |
| 应用退出、目录里只少了第一个文件 | 一拍里做了太多 VFS；拆成增量作业 |
| 亮度设 300 读回 255 | 固件自己钳到 0-255；静态看调用点会以为量程是 600 |

## 三、页面骨架（RW6 版）

照抄 `work/p65_chaos/src/lib.rs` 里的既有页，别自创结构：

- **注册**：每页一份 29 word（116B）描述符，`[4]`=页名、`[5]`=app_id<<16|页索引、
  `[19..24]`=on_create/on_resume/on_pause/on_destroy/on_ui_destroy；
  页名用 `b"chaosxxx\0"`（`P_*` 静态串），`PAGE_N` 与 `pnames` 数组长度必须一致。
- **建页**：`FW_CARD_NEW(ROOT,0)` 建页面对象 → 一串清样式/置状态 → 建**内容容器**
  （`FW_SCROLLBAR(content,0x0C)` 这发不能少）→ 行挂进内容容器。
- **行**：`row(parent, y, h, 主标签, 副标签, 点击回调下标, slot)`；
  副标签 224 宽放不下长文案时用 `row_w(..., key_w, val_w)`。
- **标题**：`TITLE_CREATE(page, 标题, 有无返回键, 0, 0)`；根页没有返回键，子页有。
- **跳转**：`goto_page((APP_ID<<16)|页索引, 0, 0, 0)`；页栈动画由固件给。
- **取数**：在 `build_page` 里取好写进静态缓冲，行里只放指针；
  静态数组元素取地址要用 `addr_of!(ARR[i])`，**不能用 `.as_ptr()`**（那取到临时副本）。

## 四、验收

- 截图看观感（不是读数）：`C.shot('名字')` 落在 `build/`。
- 数值型功能看**页面读数**（模拟器里背光不影响帧缓冲，画面明暗验不了）。
- 回归：主页 → 每个子页 → 返回，逐页截图；子页返回键在标题栏左上。
- **别用"连续压很多页"做回归**：实测一口气 `goto_page` 压到第 9 页时应用直接退出
  ⇒ 页栈有深度上限。逐页进、逐页返回才安全。

## 五、出包与安装

```bash
python scripts/build_chaos_p65_installer.py    # -> dist/chaos-p65-043-v1.tar
```

包内：`chaos_sup_p65.ko`（e_entry 已补）、`chaos_icon.bin`、`install.sh`、
`uninstall.sh`、`README.txt`。设备侧（NSH）：

```
tar -C /data -x /data/chaos-p65-043-v1.tar
sh /data/install.sh
```

设备侧两个硬限制（实测）：

- NuttX 的 tar **只吃 USTAR**，PAX 会报 `tar_extract_all(): Unknown error 22`；
- 设备 tar **不支持 -z**：给 `.tar` 不要 `.tar.gz`；选项写在文件名之前。

## 六、还没打通的（别重复踩）

- **开机自启**：RW6 的 /etc/init.d 在 romfs 里（只读），固件没有任何地方从 /data
  执行脚本或 insmod ⇒ 现状是"装一次、手动跑一次 install.sh"。
- **震动**：走 vela 服务框架（`vibrate_driver_start` 0x0C92B578 那种"参数结构指针"入口），
  服务句柄与消息布局没挖通。
- **表盘列表**：/data/app/watchface/builtin/<id>/ 里只有 config.json + preview.bin，
  **显示名不在本地文件**，要另找来源。
- **launcher 桌面**：Studio 的 RW6 guest 里没有 /system（rcS 的 losetup 失败），
  所以桌面图标**在模拟器里看不到**，只能读桌面记录链验证。

## 七、原生控件配方（2026-10-31 起，全部 guest 验证过）

系统页里的控件先用"类表扫描"拿全（`build/lv_class_scan.py`：扫 PSRAM，读类结构的
`+0x14` 名字串），再按类建。本机可直接用的：

| 控件 | 类地址 | 建法与要点 |
|---|---|---|
| 系统蒙皮开关 | 0x3C34D774 (52x92) | create_obj+init；初态写对象 `+0x30` 最低位再 `prop(P_WIDTH,…)` 重绘 |
| 复选框 | 0x3C224090 | 同上；只做"当前项"指示时不挂事件 |
| 进度条 | 0x3C223B58 | `lv_bar_set_value` = 0x0C784850（夹在 min `+0x38`/max `+0x3C` 之间写 `+0x34`，anim 传 0） |
| 滚轮（时间/日期页用的） | 0x3C224C98 是 LVGL roller；**时间/日期页用的是 `lvx_zoom_picker`**，见 §7.1 |

建对象统一 `create_obj(class,parent)`(0x003775BC|1) + init(0x003776B0|1)。
**函数指针漏 `|1` 就是 INVSTATE 硬 fault → panic 死循环 = 整机卡死。**

三条硬规矩（都是踩出来的）：

1. **事件只挂具体码**：`obj_add_event(o, cb, 0(ALL), 0)` 会把整页手势吃掉、页面滚不动；
   只挂 `CLICKED(7)`（或 `SCROLL(11)`）就正常。
2. **开关尺寸别硬设**：设成 20x20 会只剩一个小圆点；用类默认 52x92。
3. **开关行的点击实际命中卡片**，回调要挂在卡片那个槽（整行点都生效）。

表盘切换页数据源不是扫目录，是**读管理器链表**：全局指针字 `0x3C49762C`，
`mgr+0x48` 当前主表盘、`+0x74` 链表头、`+0x70` 哨兵；节点 `+0x04` next、`+0x08` id、
`+0x48` 内联名、`+0x88` 类型、`+0x90` in_use；取名走
`watchface_manager_get_watchface_name` 0x0CA90C7C。

### 7.1 原生滚轮 `lvx_zoom_picker`（时间/日期页，2026-10-31 全部上屏验过）

| 用途 | 入口 | 要点 |
|---|---|---|
| 建轮 | 0x0C240411 | `create(parent)`；建完**先 `fw_size`/`fw_pos` 再设范围**（标签位置在 refresh 里按当时尺寸算） |
| 设范围 | 0x0C24061D | `(p, min, max, step)`；写 +0x540=项数、+0x53C=数组 |
| 挂字符串选项 | 0x0C240741 | `(p, const char **items, count)`；**第二参必须是字符串指针数组**，传整数当场把 guest 干挂 |
| 模式 | 0x0C240435 | 1 = 循环 |
| 设初值 | 0x0C2377A9 | 内部滚动核心；`(p, index, count)`，**index 必须 < count**（核心有 `count<=index` 前置门）；循环窗由控件自己折算 |
| 取选中值 | 不用 getter | `(+0x94 首可见) + (+0xFC 窗内偏移 u8)`，再按项数取模 |

上屏要点：建轮 → 设范围 → 模式 1 就是**原生皮肤**，不需要另挂样式对象；控件**实际高度约 118**
（`fw_size` 的高度对它不生效），拖动要落在轮子自己的矩形里，按 270 高去拖会落到页面外。

### 7.2 换字体（系统美化页，2026-10-31 验过）

- 数据源：`/resource/font` 下 7 个 ttf；**只列 MiSans 一族**（`N62SFontVF` 没有中文字形，
  选中后整页中文连"返回"都变方块）。
- 建脸：`vg_font_create_named(名字, 22, 0)`（名字**不带 .ttf**，模块里 `MFONT`=0x0C2188ED）。
- 写到对象：`prop(label, 0x5A, face)`（text_font 属性号 0x5A）。
- **只写当前页的标签**：试过把别的页标签也批量写一遍，结果**输入直接失灵**（页面和文字都变了，
  返回行与标题栏箭头都不吃触摸）。跨页生效改成置"字体脏"标志、回到该页时**重建一次**，
  新标签创建时就套新脸（实测把主页中文换成 N62S 的豆腐块，传播成立）。
- 长列表的"返回"放**行 0**（末行会超出视口、点不到）；返回动作用 `page_back`：
  `FUN_00162620(param=0)` = 0x0C22_2621，参数 0 = 弹当前页（自触发探针实测 <0.5s 生效）。

## 八、导航回归（改页面必跑）

`python scripts/p65_pages_accept.py`：24 步，判据是模块自己的页号（页内切视图那步用行数），不靠截图观感。

**点行的正确姿势（2026-10-31 定案，之前随机点错行就是这三条）**：
1. **别猜滚动量**。手指甩动的实际位移不是拖动距离的 1:1（实测拖 320px，内容走 552px，
   约 1.7 倍），惯性还会再跑秒级。改为模块在 `navdbg` 构建里每拍把每张行卡片的屏幕矩形
   （`lv_obj_t+0x1C` 起 4 个 i32）发布进模块自己的 DATA 段：
   `[魔数 0xC0DE00E7, CUR, ROOT, 页根存活, 行数, 24*(x1,y1,x2,y2)]`；
   宿主机 `chaos_guest.find_magic()` 按魔数定位后一次 QMP 读全拿，按行号点矩形中心。
   **不要用固定 scratch 页（0x3C7FF000）**：会被别的东西覆盖，实测读到整页 `0x22222222`，
   症状是 "CUR 突然变成 572662306、所有行消失"。
2. **别在甩动后立刻点**。带惯性的快滑之后第一击会被 LVGL 当成"停滚动"吞掉，不发 CLICKED；
   只看"两次快照相同"判停稳也不够（缓动末段整秒不动，看着像停了）。
   用**慢拖 + 收尾按住 0.45s 再抬手**（无甩动），并且导航步骤要"没跳就补点一次"。
3. **返回键同理**：翻页动画没走完时点会被吞，返回必须"按了等 CUR 真变，没变就补按"。

其它两条实测事实：
- 页根 `spec_attr` 被清空 = 应用被系统切走（空闲约 38s 就会），脚本报「应用已切走」；
  所以回归要边点边读，不能长时间干等。
- 快照只在 `navdbg` 构建里存在，正式包体积不受影响（本机 TEXT 15436 / DATA 11212 复验过）。

## 八点五、guest 掉线自助恢复（2026-10-31 实测）

- `NSH reboot` 之后实例会换一个 runDir，`nativeResourcesReady` 可能一直卡 False —— **别只看
  这个标志**；`run.py` 里新加的 `ensure_guest()` 是"NSH 能应答 + 日志尾有 ap>"两个条件轮询，用它。
- 强杀 qemu 后立刻 start 可能报 `starting-failed`（模型 state=error，错误文案写着"固件包兼容性"）。
  这是**瞬时**的：等没有残留 qemu 进程后重发 `/api/instances/start`
  （`{instance, skipBinding:true}`），再轮询 `/api/instances` 里模型的 `state`，一般 4~5 次
  （每次 6s）就到 `running`。不用动任何文件。
- **两个 Studio 进程会抢同一份 state**：症状就是 `starting-failed`（"固件包兼容性"），
  而且新 run 的 `boot-uart.log` 是 0 字节。先 `Get-Process | ? ProcessName -match 'MiBand'`
  看看是不是有两个，杀掉多余那个，再 start 就能起来（2026-10-31 实测）。
- Studio 本身挂了才重启应用：`%LOCALAPPDATA%` 下
  `MiBandStudio\cache\updates\0.0.2\MiBandStudio\MiBand Studio.exe`（目录名没有空格），
  起来后从 launcher.json 现读新端口。

## 九、仍未打通的共同缺口：ioctl / syscall

`rtc0`、`uorb/` 的 29 个话题、`usensor` 全是字符设备，订阅/设时间都要 ioctl；
模块按未定义符号调 libc 不行（`insmod failed: 2`）；镜像里没有可复用的
`file_ioctl` 封装。时间·日期、摇一摇轮换要落地，得先自建 svc 并定 syscall 序号。
应用隐藏则是另一条：桌面记录节点里没找到可见性字段，语义未取证。

**2026-10-31 晚更新的实测（别再重复）**：
1. **字符串引用要按 0x3C（PSRAM）域搜，不能拿"文件偏移+0x0C0C0000"当运行期地址。**
   我按后者搜 `date` 的格式串 / getopt 串 / `clock_settime`，得到"零引用"并写成"命令不在镜像里"，
   那是错的。从当前 guest 读 0x3C000000 起 4MB 才拿到真值：`clock_gettime` @0x3C195F0C、
   `%a, %b %d %H:%M:%S %Y` @0x3C195F2C、`clock_settime` @0x3C195F7C、`timers/rtc.c` @0x3C16AB60、
   `/dev/rtc%d` @0x3C16AB70；它们在镜像里的引用点是 0x4F4Exx..0x4F53xx 与 0x41B4xx..0x41B6xx。
   ⇒ `date`/时钟那族在 XIP 0x0C5B4Exx..0x0C5B53xx；RTC 上层在 0x0C4DB4xx..0x0C4DB6xx。
2. **`build/rw6_measure/*` 的 PSRAM dump 是 10-25 那版固件，地址已漂**（同一偏移现在读到别的串），
   别再拿它查"谁引用这个串"；要查就现读 guest 内存（`qmpmem.read(..., step=8192)` 可以一次读 8KB）。
3. **RTC 不是内存映射寄存器**，是 PMU 的 ISPI 通道（权威：`_ref_miband_qemu` 的
   `pmu_rtc_model` / `rtc_base_seconds` / `miband_ispi_set_pmu_rtc_model()`）。
   "直接写 RTC 寄存器"不成立，写时钟要走 `chip/bes_rtc.c` 的 settime → ISPI 事务。
4. **`ghidra_fw.py` 的口径**：工程 BASE=0，查询用**文件偏移**；调用形式是
   `ghidra_fw.py fn <目标名> <地址>`（把地址当目标名会报"没有这个目标"）。
   另外 `fn` 会在给定地址**现场造函数**，别把函数中间的地址丢进去（会留下假函数）。
5. **写时钟入口已执行验证通过**：`clock_settime = 0x0C4C1FB4|1`、`clock_gettime = 0x0C4C205C|1`，
   `struct timespec` 是 12 字节（64 位 time_t，nsec 在 +8）。判据用固件自己的 `date -u`：
   setonly 一装，`date` 从 2026-10-31 变成 2023-11-14 22:13:26，`ret=0` ⇒ 绝对时间语义。
   **坑**：探针前两版一 insmod 就 NSH 503（整机卡死），根因是函数指针没 `|1`（EVEN 地址
   ⇒ INVSTATE 硬 fault）。探针模板在 `work/probe_clock`（`bash work/probe_clock/build.sh <feature>`，
   变体 noop / logcall / getonly / setonly；编辑 `probe_p65/src/lib.rs` 会被裸 NUL 挡住，别改它）。
   持久化未确认：整机重启会把模拟 RTC 重置成宿主时间，要判"写没写 RTC"得在同一 QEMU 实例内
   让 NuttX `reboot` 再看 `date`。
