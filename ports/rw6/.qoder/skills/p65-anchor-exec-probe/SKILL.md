---
name: p65-anchor-exec-probe
description: 在 MiBand Studio 的 redmi-watch6 guest（真实 p65/best1502p 固件）里对"已定案的固件入口地址"做执行级验证：编一个只做一件事的 Vela 模块、补 e_entry、传进 /data、NSH insmod、再用 xd/dmesg 读效果，崩了自助重启。当要给 fw_api 里的锚点从"双侧反编译对上"升级到"实机执行确认"，或怀疑某个地址一点就崩、模块装不进去、guest 卡死/503 时使用。触发词：锚点执行验证、insmod、Vela 模块、modlib、e_entry、xd、dmesg、guest 卡死、MiBand Studio 503。
---

# p65 锚点执行验证（Studio guest 版）

## 铁律

1. **一次只验一支锚点**。一个模块只调一个固件入口、不做别的事；崩一次就要重启 guest，混着调等于白测。
2. **判据只认三类**：guest 内存读回（`xd`）、固件自己的日志（`dmesg`）、`lsmod`/进程状态。
   "没崩" 只能记 `SIM_EXECUTED`，不等于语义确认。
3. **不要拿无效参数试探"应该会早退"的分支**：p65 的页面/注册表路径经常先解引用再判空，
   无效键会直接崩（`page_goto` 两支都这样）。要验就先把**真实的对象/键**从活体内存里读出来。
4. 崩了**自己恢复**，不要找人点按钮。

## 环境与协议（Studio 0.0.2）

- 机型 `miband-best1502p` 就是 RW6；产品目录 `redmi-watch6`；缓存里的 AP 镜像与仓库 `vela_ap.bin` 同 sha256。
- 管理器地址在 `%LOCALAPPDATA%\MiBandStudio\state\launcher.json` 的 `url`，**每次启动换端口**。
- 管理器 token = 管理器自己的 `<base>/config.json` 里的 `controlToken`；
  起实例：`POST <base>/api/instances/start {"instance": id, "skipBinding": true}`（约 9 秒后实例可用）。
- 实例 token = `<base>/instances/<id>/config.json` 的 `controlToken`；写操作还要带 `X-MiBand-Generation`。
- NSH：`POST <base>/instances/<id>/api/nuttx {"input": "cmd\n"}`。
  **回显经常取不到**（GET 空），结果一律读串口日志：
  `state/runs/<实例>/<run>/boot-uart.log`（路径从 qemu 命令行里的 `logfile=` 取）。
- 文件：`POST <base>/instances/<id>/api/files`，字段**平铺** `{action, …}`：
  `upload-begin{size}` → `POST api/files/upload?ticket&offset` 原始字节 →
  `start{request:{operation:"upload", path:"/data", files:[{ticket, relativePath}]}}` → 轮询 `status{id}`；
  目标已存在会停在 `conflict`，`resolve{id, choice:"replace"}`。
- NSH 自带 `xd <addr> <bytes>`、`insmod <file> <name>`、`lsmod`、`dmesg`、`md5`、`hexdump`。
  monitor（`/api/debug`）只放行 `info *`，`x/xp` 被白名单挡住 —— 读内存走 NSH 的 `xd`。

## 模块必须做的一件事：补 `e_entry`

p65 的 modlib 拿 **ELF 头 `e_entry`** 当模块入口；`rust-lld -r` 的部分链接产物 e_entry 恒为 0
⇒ insmod 一跳死在 0，表现是 **guest 卡死**（串口只留一行命令回显、NSH 与 monitor 全 503），
不是报错。补丁：把 e_entry 设为 `module_main` 的 `st_value`（节内偏移，Thumb 位已由链接器置 1）：

```bash
python work/probe_p65/patch_entry.py <module.ko>      # 默认入口符号 module_main
```

装成功后 `lsmod` 的 `INIT` 列 = text 基址 + 0x19。

## 五步

```bash
bash work/probe_p65/build.sh <feature>                 # 建一个只做一件事的探针
python work/probe_p65/patch_entry.py work/probe_p65/probe_p65.ko
python work/probe_p65/g.py put work/probe_p65/probe_p65.ko probe_x.ko
python work/probe_p65/g.py nsh "insmod /data/probe_x.ko probe_x"
python work/probe_p65/g.py nsh "xd <addr> <n>"         # 或 dmesg / lsmod
```

崩了（`nsh` 回 503）：杀这个 guest 的 qemu 进程（命令行含 `-M miband-best1502p`），
再 `python work/probe_p65/g.py` 里那条管理器 start（或 `g.py restart`）。

## 已知的安全/危险目标

| 目标 | 结论 |
|---|---|
| `brightness_set_value(v)` | 安全。判据：状态块（基址 `0x3C4965B8`）`+0x0C` 变成映射后的值；`+0x10` 不动 |
| `brightness_set_full_power(1)` | 安全。判据：状态块 `+0x18` 0→1，上限格 `+0xEC` 走手动档时保持 600 |
| `launcher_refresh_app(不存在 id)` | 不崩（走"找不到记录"分支）；要坐实语义得用真实 id 看记录变化 |
| `page_goto(...)` | **危险**：无效键、合法键 + `start_data=0` 都崩。要验先用 043 真实调用点的实参 |
| `app_launcher_add/del`、`zoom_picker_create`、`page_title_create` | 需要真实 app id / 活页对象；空参数会崩 |

## 日志与内存

- 固件日志是**延迟日志**：串口不一定立刻出，`dmesg` 才是全量。
- 我们自己的日志调用还没对上：固件里的包装是 `(level, 指针, 指针, 指针, 值…)` 形状，
  只传 `(level, fmt)` 在 dmesg 里没有输出 ⇒ 在那之前判据只用内存读回或固件自己的日志。
- 亮度状态块基址可从函数的池格现读（`xd 0x0CA65C94 4` → `0x3C4965B8`），
  交叉印证：该块 `+0x1C` 是 fb fd（19）。
- 页链表头：`xd 0x0C222E2C 8`；节点 `+0x14` 是页键、`+0x44` 是 next。
- 桌面记录链头：`xd 0x0C7EC864 8`。

## 工具

`work/probe_p65/`：`g.py`（status/nsh/tail/put/insmod/cont/restart）、`patch_entry.py`、
`build.sh`（features: 默认三锚点 / `min` / `empty` / `shaped` / `bright` / `full` / `goto` / `goto_key` / `refresh`）。

## 结果通道（验"有返回值/返回对象"的锚点）

日志形状对不上时，让模块把**返回值写进自己的 `.data`**，宿主机读回来：

```rust
#[no_mangle] pub static mut PROBE_RET: [u32; 2] = [0, 0];   // [0]=魔数, [1]=返回值
// module_main: PROBE_RET[0] = 0x52455431; PROBE_RET[1] = 被测函数(...) as u32;
```

```bash
lsmod                 # 该模块的 DATA 基址（倒数第二列）
xd <DATA基址 + PROBE_RET 的节内偏移> 8
# 读到 [0x52455431, 返回值] 就说明函数跑到了
```

PROBE_RET 的节内偏移从 .ko 的符号表取（`st_value`，`.data` 段地址已知）。
拿到返回对象后再读它的头几个字做"像不像那个东西"的核对
（例如 `lv_font_t`：三个代码指针 + 行高 + base_line）。

## 崩了怎么看现场（QMP 后门）

Studio 的 `/api/debug` 只放行 `info *`，**但实例 qemu 的 QMP 端口接受任意 `human-monitor-command`**：

```python
# 端口从 qemu 命令行里取：-qmp tcp:127.0.0.1:<port>
{"execute":"qmp_capabilities"} -> {"execute":"human-monitor-command",
                                  "arguments":{"command-line":"info registers"}}
# 也支持 "xp/96xb 0x3C1AB680"（物理地址按字节读）、"info mtree"
```

读到的 `PC` 若等于 **0x0C4F74E8** 就是撞了断言哨兵（不是 HardFault）：
`R0` 往往指向断言现场的字符串区（源文件/表达式），`xp` 出 ASCII 就能看出是哪一类断言。
本轮靠它定案：`app_install` 崩在 `libuv/src/unix/async` 一族 —— 它要向主循环投递，
属于**主循环上下文 API**，从 insmod 的 NSH 线程调必崩（与参数无关）。

## 本轮踩到的操作坑（2026-10-24，都已复现过）

1. **Git Bash 会把 `/data/x.ko` 变成 `C:/Program Files/Git/data/x.ko`**，于是 `insmod` 报
   `too many arguments`（看着像参数个数问题，其实是路径被拆成三段）。跑 NSH 命令时加
   `MSYS_NO_PATHCONV=1 python work/probe_p65/g.py insmod /data/x.ko name`。
2. `insmod` **必须两个参数**（文件 + 模块名）；只给文件报 `missing required argument(s)`。
3. 探针崩了之后 guest 会**自己 board_reset**；复位后的第一段启动常常停在
   `[ap]system power on ota test....`，这时**再 kill+start 一次**才能回到 `ap>`。
4. 复位期间（`nativeResourcesReady=False`）调文件接口会 **409 Conflict**；等
   `nativeResourcesReady=True` 且日志末尾有 `ap>` 再传。
5. 判"崩没崩"别看 `/api/machine` 的 `powerState`（崩了也显示 running），看**日志里命令后面
   有没有 `ap>` 提示符**；没有 = 卡住/复位。
6. **在 NSH 线程里调 LVGL 结构函数目前四档全崩**（`lv_obj_create` 单/双参、
   带活体父的 `create_obj`、只调 `create_obj` 都崩；`min` 对照正常）。
   在拿到"只读档"或"主循环回调档"的判据之前，**不要继续往探针里加 LVGL 调用**。
7. `lv_obj_class_init_obj` 是 **两参** `(obj, ctx)`：`ctx=0` 才读默认值（file 0x117D0）。
   按单参声明会让 `r1` 保持调用前的垃圾值。


## 已验证可用的锚点（2026-10-24，均在 guest 里跑过）

```
create_obj       0x003775BD   (class, parent) -> obj     SRAM 真身；走 XIP 壳会返回小整数
init_obj         0x003776B1   (obj, 0)                   SRAM 真身
lv_label_class   0x0C314608                              flash；class 结构 +0x14 是标签串 "label"
lv_label_set_text 0x0C78C879  (label, cstr)              文本在 label+0x34
lv_obj_delete    0x0C1A9E57  (obj)                       发 LV_EVENT_DELETE(0x23)
lv_obj_add_flag  0x00387B99  (obj, flag)                 标志字在 obj+0x2C
lv_obj_add_event_cb 0x0C2570BD (obj, cb, filter, udata)  描述符 +0x10=udata、+0x14=filter
set_local_style_prop 0x00378965 (obj, prop, value, sel)  样式/几何统一入口
lv_timer_create  0x0C7414D9  (cb, period, ud)            回调在主循环里跑
app_install      0x0C2418A9  (desc, pages, count)        **必须在主循环线程里调**
```

样式属性号（从固件自己的 setter 抠，**不要按枚举顺序猜**）：
`W=1 H=4 X=7 Y=8 ALIGN=9`、`BG_COLOR=0x1C BG_OPA=0x1D`、**文本块 0x58..0x5E，TEXT_FONT=0x5A**。

## 主循环上下文怎么拿

`lv_timer_create(cb, period, 0)` 一句就够；回调里不要做重活（一拍只做一步），
用模块 `.data` 里的计数器控制"只做一次"。**`app_install` 只能在回调里调**
（NSH 线程里会崩在 `libuv/src/unix/async.c:212`，errno 9 = EBADF）。

## 注册自己的页

克隆 app19 的 `pages[2]` 页描述符（`lookup(19)` → 记录 `+0x30` 是页数组 `[count,p0,p1]`）：
改 `+0x10` 名字串、`+0x14` 页键，**清零 `+0x40/+0x44`**（那是别人的链），再
`install(desc, 自己的页数组, 1)`。`pages[1]` 不是页描述符（拿它注册会撞 `hashmap.c:147` 空键）。

## 调用口径（踩过）

- file ≥ 0x2FDC4 的 flash-only 段：XIP 地址可直接调。
- 经跳转壳 / SRAM 到达的：用 `scripts/p65_thunk_resolve.py` 解析出的真身（SRAM/RAMX）。
  file < 0x2FDC4 的段**不在 PSRAM 拷贝范围**，别自己拼别名。
- 断言现场：`R0` = 源文件串、`R1` = 行号；`VA−0x3BFD023C` 换成 file 偏移读串即可认文件。
- **每 5~6 档探针 kill+start 一次**：连续建对象会把 NuttX 堆耗尽
  （`mm_heap/mm_malloc.c:333 Allocation failed`），之后全是假崩。
